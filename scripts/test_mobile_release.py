"""Exercise release orchestration with disposable tools/APIs; never upload."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import yaml

ROOT = Path(__file__).resolve().parents[1]
SHA = "0123456789abcdef0123456789abcdef01234567"


def load_script(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def workflow(name):
    # BaseLoader keeps GitHub's YAML 1.2 'on' key from becoming a boolean.
    return yaml.load((ROOT / ".github/workflows" / name).read_text(), Loader=yaml.BaseLoader)


def executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.bin = self.directory / "bin"
        self.bin.mkdir()
        self.env = {
            **os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
            "GITHUB_SHA": SHA, "GITHUB_REPOSITORY": "joswayski/dbm",
            "GITHUB_SERVER_URL": "https://github.com", "GITHUB_API_URL": "https://api.github.com",
            "GITHUB_OUTPUT": str(self.directory / "output"), "GITHUB_RUN_NUMBER": "9",
            "GITHUB_RUN_ATTEMPT": "2", "REQUESTED_SHA": "", "TESTED": "1",
            "ANCESTOR": "0", "CALLS": str(self.directory / "calls"),
        }
        executable(self.bin / "git", '#!/bin/sh\nexit "$ANCESTOR"\n')
        executable(self.bin / "gh", '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\necho "$TESTED"\n')

    def run_step(self, script, **env):
        return subprocess.run(["bash", "-c", script], env={**self.env, **env},
                              text=True, capture_output=True, cwd=self.directory)

    def test_resolve_rejects_untested_non_main_or_malformed_commits(self):
        release = workflow("mobile-release.yml")
        job = release["jobs"]["resolve"]
        self.assertEqual(job["if"], "github.ref == 'refs/heads/main'")
        self.assertEqual(job["permissions"]["actions"], "read")
        script = job["steps"][1]["run"]
        for env in ({"TESTED": "0"}, {"ANCESTOR": "1"}, {"REQUESTED_SHA": SHA.upper()},
                    {"REQUESTED_SHA": "main; touch unexpected"}, {"GITHUB_RUN_ATTEMPT": "100"}):
            with self.subTest(env=env):
                self.assertNotEqual(self.run_step(script, **env).returncode, 0)
                self.assertFalse((self.directory / "output").exists())
        result = self.run_step(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.directory / "output").read_text(), f"sha={SHA}\nandroid_build=902\n")
        self.assertIn(f"head_sha={SHA}&branch=main&event=push&status=success", (self.directory / "calls").read_text())
        self.assertEqual(self.run_step(script, GITHUB_RUN_ATTEMPT="3").returncode, 0)
        self.assertTrue((self.directory / "output").read_text().endswith("android_build=903\n"))

    def test_publishing_cannot_replace_the_desktop_latest_release(self):
        release = workflow("mobile-release.yml")
        script = release["jobs"]["publish"]["steps"][1]["run"]
        result = self.run_step(script, RELEASE_SHA=SHA)
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = (self.directory / "calls").read_text().splitlines()
        self.assertEqual(len(calls), 3)
        self.assertTrue(calls[0].startswith("release view mobile-latest "))
        self.assertTrue(calls[1].startswith("release delete mobile-latest "))
        self.assertIn("--prerelease --latest=false", calls[2])
        self.assertIn(f"--target {SHA}", calls[2])
        self.assertEqual(release["jobs"]["publish"]["needs"], ["resolve", "android"])

    def test_play_rollout_requires_dbm_credentials_without_shared_fallback(self):
        step = workflow("mobile-release.yml")["jobs"]["android"]["steps"][-2]
        self.assertNotIn("if", step)
        executable(self.bin / "aws", '#!/bin/sh\nprintf "%s" "$SIGNING_SECRET"\n')
        executable(self.bin / "python3", '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\n')
        account = json.dumps({"client_email": "dbm-release@example.invalid"})
        caper = {"service_account_json": json.dumps({"client_email": "caper-release@example.invalid"})}
        secret = json.dumps({"dbm_google_play": {"service_account_json": account}, "google_play": caper})
        result = self.run_step(step["run"], SIGNING_SECRET=secret,
                               RUNNER_TEMP=str(self.directory), DBM_BUILD_NUMBER="902")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.directory / "play.json").read_text().strip(), account)
        self.assertEqual((self.directory / "play.json").stat().st_mode & 0o777, 0o600)
        self.assertIn("scripts/play-upload.py dist/mobile/play/DBM-Android-Play.aab",
                      (self.directory / "calls").read_text().splitlines())

        (self.directory / "calls").unlink()
        result = self.run_step(step["run"], SIGNING_SECRET=json.dumps({"google_play": caper}),
                               RUNNER_TEMP=str(self.directory), DBM_BUILD_NUMBER="902")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.directory / "calls").exists())

    def configure_curl(self):
        executable(self.bin / "curl", '''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
if "/pulls?" in args[-1]:
    print(json.dumps([{"title": "Native apps; $(touch unsafe)", "body": "Test DBM\\n@everyone"}]))
else:
    with open(os.environ["CALLS"], "w") as out:
        json.dump({"url": args[-1], "payload": json.loads(args[args.index("--data") + 1])}, out)
''')
        self.env.update(DEPLOY_NOTIFICATION_WEBHOOK_URL="https://discord.com/api/webhooks/123/fake-token",
                        GITHUB_TOKEN="fixture", GIT_SHA=SHA, COMMIT_MESSAGE="Fallback",
                        CI_RUN_URL="https://github.com/joswayski/dbm/actions/runs/123")

    def test_ready_notification_pins_the_sha_and_disables_mentions(self):
        self.configure_curl()
        ready = workflow("mobile-ready.yml")
        self.assertEqual(ready["on"]["workflow_run"]["workflows"], ["Mobile development builds"])
        self.assertIn("conclusion == 'success'", ready["jobs"]["notify"]["if"])
        self.assertIn("event == 'push'", ready["jobs"]["notify"]["if"])
        result = self.run_step(ready["jobs"]["notify"]["steps"][0]["run"])
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads((self.directory / "calls").read_text())["payload"]
        self.assertEqual(payload["allowed_mentions"], {"parse": []})
        self.assertEqual(payload["embeds"][0]["title"], "Native apps; $(touch unsafe)")
        self.assertFalse((self.directory / "unsafe").exists())
        buttons = payload["components"][0]["components"]
        self.assertEqual(buttons[0]["custom_id"], f"production-deploy:v1:dbm-mobile:{SHA}")
        self.assertTrue(buttons[1]["url"].endswith("/mobile-release.yml"))

    def test_result_controls_and_rejected_foreign_run_url(self):
        self.configure_curl()
        command = f'bash "{ROOT}/scripts/update-discord-mobile-release.sh" --git-sha {SHA} ' \
                  '--message-id 123456789012345678 --started-at 1791080000 --finished-at 1791080073 '
        for result, label, disabled in [("success", "Released", True), ("failure", "Retry release", False),
                                        ("cancelled", "Check GitHub", True)]:
            run = self.run_step(command + f"--result {result} --run-url https://github.com/joswayski/dbm/actions/runs/123")
            self.assertEqual(run.returncode, 0, run.stderr)
            payload = json.loads((self.directory / "calls").read_text())["payload"]
            button = payload["components"][0]["components"][0]
            self.assertEqual((button["label"], button["disabled"]), (label, disabled))
            self.assertEqual(button["custom_id"], f"production-deploy:v1:dbm-mobile:{SHA}")
            self.assertIn("Duration: 1m 13s", payload["content"])
        (self.directory / "calls").unlink()
        run = self.run_step(command + "--result success --run-url https://github.com/joswayski/caper/actions/runs/123")
        self.assertNotEqual(run.returncode, 0)
        self.assertFalse((self.directory / "calls").exists())

    def test_ios_archive_uses_release_device_signing_and_explicit_build_number(self):
        ios = self.directory / "apps/native/ios"
        ios.mkdir(parents=True)
        for name in ["prepare.sh", "upload-testflight.sh"]:
            shutil.copy(ROOT / "apps/native/ios" / name, ios / name)
        (ios / ".build/xcodegen-21ac9944b0ab546a07422dbed86f33dd2ebd76f8/.git").mkdir(parents=True)
        archive = self.directory / "target/aarch64-apple-ios/release/libdbm_native_bridge.a"
        archive.parent.mkdir(parents=True)
        archive.write_bytes(b"fixture")
        for name in ["cargo", "rustup", "swift"]:
            executable(self.bin / name, '#!/bin/sh\nexit 0\n')
        executable(self.bin / "git", '#!/bin/sh\nif [ "$3" = rev-parse ]; then echo 21ac9944b0ab546a07422dbed86f33dd2ebd76f8; fi\n')
        executable(self.bin / "xcodebuild", '''#!/usr/bin/env python3
import json, os, plistlib, sys
args = sys.argv[1:]
entry = {"args": args}
if "-exportOptionsPlist" in args:
    with open(args[args.index("-exportOptionsPlist") + 1], "rb") as source:
        entry["options"] = plistlib.load(source)
with open(os.environ["CALLS"], "a") as out:
    out.write(json.dumps(entry) + "\\n")
''')
        key = self.directory / "fixture.p8"
        key.write_text("fake key; never used to authenticate")
        run = self.run_step(f'bash "{ios}/upload-testflight.sh"', APPLE_TEAM_ID="TESTTEAM",
                            NOTARY_KEY_PATH=str(key), NOTARY_KEY_ID="TESTKEY", NOTARY_ISSUER="TESTISSUER",
                            BUILD_NUMBER="9.2", DBM_IOS_BUNDLE_ID="app.dbm.ios")
        self.assertEqual(run.returncode, 0, run.stderr)
        calls = [json.loads(line) for line in (self.directory / "calls").read_text().splitlines()]
        self.assertEqual(len(calls), 2)
        self.assertIn("Release", calls[0]["args"])
        self.assertIn("generic/platform=iOS", calls[0]["args"])
        self.assertIn("DBM_IOS_BUNDLE_ID=app.dbm.ios", calls[0]["args"])
        self.assertIn("CURRENT_PROJECT_VERSION=9.2", calls[0]["args"])
        self.assertEqual(calls[0]["args"][-1], "archive")
        for call in calls:
            self.assertIn("-allowProvisioningUpdates", call["args"])
            self.assertIn("-authenticationKeyPath", call["args"])
            self.assertNotIn("CODE_SIGNING_ALLOWED=NO", call["args"])
        self.assertEqual(calls[1]["options"], {"destination": "upload", "method": "app-store-connect",
                         "teamID": "TESTTEAM", "signingStyle": "automatic",
                         "manageAppVersionAndBuildNumber": False, "uploadSymbols": True})
        self.assertFalse(Path(calls[0]["args"][calls[0]["args"].index("-archivePath") + 1]).parent.exists())


class TesterDistributionTests(unittest.TestCase):
    def test_testflight_without_testers_does_not_report_delivery_success(self):
        script = load_script("testflight-distribute")
        with patch.dict(os.environ, {"BUILD_NUMBER": "9.2"}, clear=True), \
             patch.object(script, "call", side_effect=[
                 (200, {"data": [{"id": "app"}]}),
                 (200, {"data": [{"id": "build", "attributes": {"processingState": "VALID"}}]}),
                 (200, {"data": []}),
             ]):
            with self.assertRaisesRegex(SystemExit, "No TestFlight tester groups"):
                script.main()

    def test_testflight_waits_for_exact_dbm_build_and_assigns_external_group(self):
        script = load_script("testflight-distribute")
        replies = [
            (200, {"data": [{"id": "app"}]}),
            (200, {"data": []}),
            (200, {"data": [{"id": "build", "attributes": {"processingState": "VALID"}}]}),
            (200, {"data": [{"id": "locale", "attributes": {"locale": "en-US"}}]}),
            (200, {}),
            (200, {"data": [
                {"id": "internal", "attributes": {"isInternalGroup": True, "hasAccessToAllBuilds": True}},
                {"id": "external", "attributes": {"isInternalGroup": False}}]}),
            (204, {}), (201, {}),
        ]
        with patch.dict(os.environ, {"BUILD_NUMBER": "9.2", "WHAT_TO_TEST": "Test TLS"}, clear=True), \
             patch.object(script, "call", side_effect=replies) as call, patch.object(script.time, "sleep") as sleep:
            script.main()
        self.assertEqual(call.call_args_list[0].kwargs["query"], {"filter[bundleId]": "app.dbm.ios"})
        self.assertEqual(call.call_args_list[1].kwargs["query"]["filter[version]"], "9.2")
        sleep.assert_called_once_with(30)
        self.assertEqual(call.call_args_list[-2].args, ("POST", "/betaGroups/external/relationships/builds",
                         {"data": [{"type": "builds", "id": "build"}]}))
        self.assertEqual(call.call_args_list[-1].args[1], "/betaAppReviewSubmissions")

    def test_testflight_rejected_build_is_never_assigned(self):
        script = load_script("testflight-distribute")
        for state in ["INVALID", "FAILED"]:
            with self.subTest(state=state), patch.dict(os.environ, {"BUILD_NUMBER": "9.2"}, clear=True), \
                 patch.object(script, "call", side_effect=[(200, {"data": [{"id": "app"}]}),
                    (200, {"data": [{"id": "build", "attributes": {"processingState": state}}]})]) as call:
                with self.assertRaisesRegex(SystemExit, "rejected build"):
                    script.main()
                self.assertEqual(call.call_count, 2)

    def test_play_targets_dbm_internal_and_only_retries_draft_errors(self):
        script = load_script("play-upload")
        with tempfile.TemporaryDirectory() as temporary:
            key = Path(temporary) / "account.json"
            key.write_text("{}")
            bundle = Path(temporary) / "bundle.aab"
            bundle.write_bytes(b"fixture bundle")
            for error, succeeds in [("Only draft releases allowed", True), ("Permission denied", False)]:
                replies = [(200, {"id": "edit"}), (200, {"versionCode": 902}),
                           (400, {"errors": [error]}), (200, {}), (200, {})]
                with self.subTest(error=error), patch.dict(os.environ, {"GOOGLE_PLAY_SERVICE_ACCOUNT": str(key)}, clear=True), \
                     patch.object(script.sys, "argv", ["play-upload.py", str(bundle)]), \
                     patch.object(script, "access_token", return_value="fixture"), \
                     patch.object(script, "call", side_effect=replies) as call:
                    if succeeds:
                        script.main()
                        self.assertEqual(call.call_args_list[3].args[3],
                                         {"track": "internal", "releases": [{"versionCodes": ["902"], "status": "draft"}]})
                        self.assertTrue(call.call_args_list[-1].args[2].endswith("/edits/edit:commit"))
                    else:
                        with self.assertRaises(SystemExit):
                            script.main()
                        self.assertEqual(call.call_count, 3)
                    self.assertIn("/com.dbm.nativeapp/edits", call.call_args_list[0].args[2])
                    self.assertEqual(call.call_args_list[2].args[3]["releases"][0]["versionCodes"], ["902"])


if __name__ == "__main__":
    unittest.main()
