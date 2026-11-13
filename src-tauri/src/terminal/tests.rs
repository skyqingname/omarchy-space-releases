use super::*;
use std::os::unix::fs::PermissionsExt;

// Execute the actual remote command strings against a synthetic guest HOME.
// No SSH server, real credentials, or model requests are involved.
struct Guest {
    home: PathBuf,
    config: TerminalConfig,
}

impl Guest {
    fn new(profile: &str) -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../evidence/login-environment-tests")
            .join(uuid::Uuid::new_v4().to_string()).join("访客's home");
        fs::create_dir_all(home.join(".claude")).unwrap();
        let home = home.canonicalize().unwrap();
        let config = config::ProductConfig::load().terminal;
        let cwd = home.join(&config.working_directory);
        fs::create_dir_all(&cwd).unwrap();
        fs::write(home.join(".bash_profile"), profile).unwrap();
        fs::write(home.join(&config.claude_state_path), json!({
            "hasCompletedOnboarding": true,
            "projects": {cwd.to_string_lossy().as_ref(): {"hasTrustDialogAccepted": true}},
        }).to_string()).unwrap();
        Self { home, config }
    }

    fn command(&self, remote: &str) -> Command {
        let mut command = Command::new("bash");
        command.args(["-c", remote]).env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::var_os("PATH").expect("test executable search path"))
            .env("XDG_RUNTIME_DIR", &self.home)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    }

    fn status(&self) -> (Value, String) {
        let session = uuid::Uuid::new_v4().to_string();
        let mut child = self.command(&follower_command(&self.config, &session)).spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let mut records = BufReader::new(child.stdout.take().unwrap());
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let result = (|| -> Result<Value, String> {
                // This is also the production handshake: a profile banner must
                // never occupy the first stdout line instead of gotocs_ready.
                prepare(input, &mut records)?;
                loop {
                    let mut line = String::new();
                    if records.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                        return Err("follower exited before reporting status".into());
                    }
                    let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                    if let Some(status) = value.get("gotocs_status") { return Ok(status.clone()); }
                }
            })();
            let _ = sender.send(result);
        });
        let result = receiver.recv_timeout(Duration::from_secs(10));
        let _ = child.kill();
        let _ = child.wait();
        reader.join().unwrap();
        let mut errors = String::new();
        child.stderr.take().unwrap().read_to_string(&mut errors).unwrap();
        (result.expect("follower status timeout").expect("follower status"), errors)
    }

    fn claude_sees_credential(&self) -> bool {
        let relative = ".local/bin/claude-env-probe";
        let probe = self.home.join(relative);
        fs::create_dir_all(probe.parent().unwrap()).unwrap();
        fs::write(&probe, "#!/bin/bash\nif [[ -n ${ANTHROPIC_API_KEY:-}${ANTHROPIC_AUTH_TOKEN:-} ]]; then echo credential-present; else echo credential-absent; fi\n").unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o700)).unwrap();
        let remote = claude_command(&self.config, relative,
            &uuid::Uuid::new_v4().to_string(), "/unused-settings.json");
        let output = self.command(&remote).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().lines().last() == Some("credential-present")
    }
}

#[test]
fn login_profile_credentials_reach_both_follower_and_claude() {
    for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] {
        let guest = Guest::new(&format!("printf 'login profile banner\\n'\nexport {key}='synthetic-test-credential'\n"));
        assert!(guest.claude_sees_credential());
        let (status, errors) = guest.status();
        assert_eq!(status, json!({"signed_in": true, "onboarded": true, "trusted": true}));
        assert!(errors.contains("login profile banner"));
        assert!(!errors.contains("synthetic-test-credential"));
    }
}

#[test]
fn login_profile_without_credentials_stays_signed_out() {
    let guest = Guest::new("printf 'login profile banner\\n'\nexport ANTHROPIC_BASE_URL=https://example.invalid\n");
    assert!(!guest.claude_sees_credential());
    let (status, errors) = guest.status();
    assert_eq!(status, json!({"signed_in": false, "onboarded": true, "trusted": true}));
    assert!(errors.contains("login profile banner"));
}
