"""Startup resets must retry on the same path without accepting an invalid exit."""
import ast
import io
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock
import urllib.error
import urllib.request

SOURCE = Path(__file__).resolve().parents[1] / 'guest/provision.py'
DEFINITIONS = [node for node in ast.parse(SOURCE.read_text()).body
               if isinstance(node, (ast.FunctionDef, ast.ClassDef))
               and node.name in ('ProvisionError', 'NodeError', 'verify_network')]


class NetworkProbeTest(unittest.TestCase):
    def context(self, proxy_results, tun_results):
        tun, proxy = Mock(), Mock()
        tun.open.side_effect, proxy.open.side_effect = tun_results, proxy_results
        request = SimpleNamespace(build_opener=Mock(side_effect=[tun, proxy]),
                                  ProxyHandler=urllib.request.ProxyHandler,
                                  Request=urllib.request.Request)
        context = {'urllib': SimpleNamespace(request=request, error=urllib.error),
                   'time': SimpleNamespace(sleep=Mock()), 'wait_network_ready': Mock(), 'command': Mock(),
                   'CONFIG': {'network': {'mixed_port': 2080, 'probe_url': 'https://example.com/',
                                          'probe_attempts': 3, 'probe_retry_seconds': 1,
                                          'probe_timeout_seconds': 30},
                              'subscription': {'user_agent': 'test'}}}
        exec(compile(ast.Module(body=DEFINITIONS, type_ignores=[]), str(SOURCE), 'exec'), context)
        return context, proxy, tun

    def test_reset_retries_same_proxy_and_records_attempts(self):
        context, proxy, tun = self.context(
            [urllib.error.URLError('connection reset'), io.BytesIO(b'ip=192.0.2.1\nloc=US\n')],
            [io.BytesIO(b'ip=192.0.2.1\nloc=US\n')])
        result, address = context['verify_network']()
        self.assertEqual(address, '192.0.2.1')
        self.assertEqual(result['network_probe_attempts'], {'显式代理': 2, 'TUN': 1})
        self.assertEqual(proxy.open.call_count, 2)
        self.assertEqual(tun.open.call_count, 1)

    def test_persistent_failure_never_falls_back_to_another_path(self):
        context, proxy, tun = self.context([urllib.error.URLError('offline')] * 3, [])
        with self.assertRaisesRegex(context['NodeError'], '3 次'):
            context['verify_network']()
        self.assertEqual(proxy.open.call_count, 3)
        tun.open.assert_not_called()

    def test_different_exits_and_invalid_payloads_never_pass(self):
        for proxy_body, tun_body in [(b'ip=192.0.2.1\nloc=US\n', b'ip=192.0.2.2\nloc=US\n'),
                                     (b'not a trace', b'ip=192.0.2.1\nloc=US\n')]:
            with self.subTest(proxy_body=proxy_body):
                context, _, _ = self.context([io.BytesIO(proxy_body)], [io.BytesIO(tun_body)])
                with self.assertRaises(context['ProvisionError']):
                    context['verify_network']()


if __name__ == '__main__':
    unittest.main()
