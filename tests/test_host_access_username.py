"""Check the real guest SSH config with a Windows username containing spaces."""
import ast
import json
from pathlib import Path
import re
import subprocess
import types
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'guest/provision.py'
TREE = ast.parse(SOURCE.read_text())
DEFINITIONS = [n for n in TREE.body if isinstance(n, ast.FunctionDef) and n.name in ('managed_block', 'configure_host_access')]

class HostUsernameTest(unittest.TestCase):
    def test_windows_username_survives_json_config_and_ssh_parser(self):
        home = ROOT / 'evidence/2026-11-04-windows-host-ssh/guest-fixtures' / str(uuid.uuid4())
        (home / '.ssh').mkdir(parents=True)
        cfg = json.loads((ROOT / 'product.config.json').read_text())
        key = home / '.ssh' / cfg['host_access']['key_comment']
        key.write_text('unused-test-fixture')
        key.with_name(key.name + '.pub').write_text('ssh-ed25519 test omarchy-space-host')
        # Only chown is stubbed: all text is produced by the real guest functions.
        context = {'CONFIG':cfg, 'Path':Path, 're':re, 'os':types.SimpleNamespace(chown=lambda *args: None)}
        exec(compile(ast.Module(body=DEFINITIONS,type_ignores=[]), str(SOURCE), 'exec'),context)
        account = types.SimpleNamespace(pw_dir=str(home), pw_name='omarchy', pw_uid=1000, pw_gid=1000)
        payload = json.loads(json.dumps({'user':'Example User','platform':'windows-x86_64'}))
        result = context['configure_host_access'](account,payload)
        text = (home / '.ssh/config').read_text()
        self.assertIn('User "Example User"',text)
        parsed = subprocess.run(['/usr/bin/ssh','-G','-F',str(home / '.ssh/config'),'host'],check=True,text=True,capture_output=True).stdout
        self.assertIn('user Example User\n',parsed)
        self.assertIn('hostname 10.0.2.2\n',parsed)
        self.assertIn('port 22\n',parsed)
        self.assertTrue(result['host_access_active'])

if __name__ == '__main__': unittest.main()
