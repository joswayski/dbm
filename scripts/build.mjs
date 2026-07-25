import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const APP_NAME = "DBM";
const BINARY_NAME = "dbm";
const APP_BUNDLE = `${APP_NAME}.app`;
const APPLICATIONS_APP = join("/Applications", APP_BUNDLE);
const RELEASE_ROOT = join(ROOT, "target/release");
const BUNDLE_ROOT = join(RELEASE_ROOT, "bundle");
const BUILT_APP = join(BUNDLE_ROOT, "macos", APP_BUNDLE);
const BUILT_EXECUTABLE = join(
  RELEASE_ROOT,
  process.platform === "win32" ? `${BINARY_NAME}.exe` : BINARY_NAME,
);

const environment = { ...process.env };
const isMac = process.platform === "darwin";
const isWindows = process.platform === "win32";
const skipInstall = environment.DBM_SKIP_INSTALL === "1";
const openAfterInstall = environment.DBM_OPEN_AFTER_INSTALL !== "0";
let usingAdHocSignature = false;

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    encoding: "utf8",
    stdio: options.stdio ?? "pipe",
    env: options.env ?? process.env,
    cwd: options.cwd,
  });
}

function log(message) {
  console.log(message);
}

function commandError(command, result) {
  const detail = result.error?.message || result.stderr?.trim();
  return new Error(
    detail ? `${command} failed: ${detail}` : `${command} failed with status ${result.status}`,
  );
}

function runChecked(command, args, options = {}) {
  const result = run(command, args, options);
  if (result.error || result.status !== 0) {
    throw commandError(command, result);
  }
  return result;
}

function npmCliPath() {
  const candidates = [
    environment.npm_execpath,
    join(dirname(process.execPath), "node_modules/npm/bin/npm-cli.js"),
  ];
  return candidates.find((candidate) => candidate && existsSync(candidate));
}

function processIsRunning(name) {
  return run("/usr/bin/pgrep", ["-x", name]).status === 0;
}

function mountedBuildDevices(hdiutilOutput) {
  const bundlePrefix = `${BUNDLE_ROOT}/`;
  const devices = new Set();
  for (const image of hdiutilOutput.split(/\n={10,}\n/u)) {
    const imagePath = image.match(/^image-path\s+:\s+(.+)$/mu)?.[1];
    const device = image.match(/^(\/dev\/disk\d+)\s/mu)?.[1];
    if (imagePath?.startsWith(bundlePrefix) && device) {
      devices.add(device);
    }
  }
  return [...devices];
}

function detachMountedBuildImages() {
  const info = run("/usr/bin/hdiutil", ["info"]);
  if (info.error || info.status !== 0) {
    console.warn("Could not inspect mounted disk images; continuing with the build.");
    return;
  }

  const devices = mountedBuildDevices(info.stdout ?? "");
  if (devices.length === 0) {
    return;
  }

  log(`Unmounting ${devices.length} stale DBM build disk image${devices.length === 1 ? "" : "s"}…`);
  for (const device of devices) {
    const detached = run("/usr/bin/hdiutil", ["detach", device]);
    if (detached.status === 0) {
      continue;
    }
    const forced = run("/usr/bin/hdiutil", ["detach", "-force", device]);
    if (forced.status !== 0) {
      console.warn(`Could not unmount stale build disk image ${device}; continuing.`);
    }
  }
}

function findAppleDevelopmentIdentity() {
  const result = run("/usr/bin/security", ["find-identity", "-v", "-p", "codesigning"]);
  if (result.status !== 0) {
    return null;
  }

  const identities = [...(result.stdout ?? "").matchAll(
    /^\s*\d+\)\s+[0-9A-Fa-f]+\s+"([^"]+)"/gmu,
  )].map((match) => match[1]);
  return (
    identities.find(
      (identity) =>
        identity.startsWith("Apple Development:") || identity.startsWith("Mac Developer:"),
    ) ?? null
  );
}

function quitRunningDbm() {
  const processNames = [APP_NAME, BINARY_NAME];
  if (!processNames.some(processIsRunning)) {
    return;
  }

  log("Quitting any running DBM instance…");
  run("/usr/bin/osascript", ["-e", `tell application "${APP_NAME}" to quit`]);
  spawnSync("/bin/sleep", ["0.4"], { stdio: "ignore" });

  for (const name of processNames) {
    if (processIsRunning(name)) {
      run("/usr/bin/killall", [name]);
    }
  }
  spawnSync("/bin/sleep", ["0.2"], { stdio: "ignore" });

  for (const name of processNames) {
    if (processIsRunning(name)) {
      run("/usr/bin/killall", ["-9", name]);
    }
  }
  spawnSync("/bin/sleep", ["0.2"], { stdio: "ignore" });

  const stillRunning = processNames.filter(processIsRunning);
  if (stillRunning.length > 0) {
    throw new Error(`Could not stop running DBM process: ${stillRunning.join(", ")}`);
  }
}

function stopRunningWindowsBuild() {
  const powershell = environment.SystemRoot
    ? join(environment.SystemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
    : "powershell.exe";
  const script = `
$target = [System.IO.Path]::GetFullPath($env:DBM_BUILD_TARGET_EXE)
$matching = @(Get-Process -Name dbm -ErrorAction SilentlyContinue | Where-Object {
  $_.Path -and [System.IO.Path]::GetFullPath($_.Path) -eq $target
})
foreach ($process in $matching) {
  Stop-Process -Id $process.Id -Force -ErrorAction Stop
  Wait-Process -Id $process.Id -Timeout 5 -ErrorAction SilentlyContinue
}
[Console]::Out.Write($matching.Count)
`;
  const result = runChecked(
    powershell,
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
    {
      env: {
        ...environment,
        DBM_BUILD_TARGET_EXE: BUILT_EXECUTABLE,
      },
    },
  );
  const stopped = Number.parseInt(result.stdout.trim(), 10);
  if (!Number.isInteger(stopped) || stopped < 0) {
    throw new Error("PowerShell returned an invalid process count.");
  }
  if (stopped > 0) {
    log(`Stopped ${stopped} running checkout cop${stopped === 1 ? "y" : "ies"} of DBM.`);
  }
}

function installToApplications() {
  if (!existsSync(BUILT_APP)) {
    throw new Error(`Built app not found at ${BUILT_APP}`);
  }

  log(`Installing ${APP_BUNDLE} → ${APPLICATIONS_APP}…`);
  runChecked("/bin/rm", ["-rf", APPLICATIONS_APP]);
  runChecked("/usr/bin/ditto", [BUILT_APP, APPLICATIONS_APP]);
  run("/usr/bin/xattr", ["-dr", "com.apple.quarantine", APPLICATIONS_APP]);
  log(`Installed → ${APPLICATIONS_APP}`);
}

function collectBundleArtifacts(directory) {
  if (!existsSync(directory)) {
    return [];
  }

  const artifacts = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name.endsWith(".app")) {
        artifacts.push(path);
      } else {
        artifacts.push(...collectBundleArtifacts(path));
      }
    } else if (/\.(?:appimage|deb|dmg|exe|msi|rpm)$/iu.test(entry.name)) {
      artifacts.push(path);
    }
  }
  return artifacts;
}

function printBuildOutputs() {
  log("");
  log("DBM build succeeded.");
  if (existsSync(BUILT_EXECUTABLE)) {
    log(`Unpackaged executable: ${BUILT_EXECUTABLE}`);
  }
  log(`Bundle output directory: ${BUNDLE_ROOT}`);

  const artifacts = collectBundleArtifacts(BUNDLE_ROOT).sort();
  if (artifacts.length > 0) {
    log("Installable artifacts:");
    for (const artifact of artifacts) {
      log(`  ${artifact}`);
    }
  }
}

function printAdHocKeychainWarning() {
  console.warn([
    "",
    "macOS Keychain notice:",
    "  This local build uses an ad-hoc signature, whose identity changes whenever DBM is rebuilt.",
    "  macOS may therefore ask for your login keychain password when the new build reads a saved",
    "  database password. That is a macOS system prompt; DBM never receives your login password.",
    "  Install an Apple Development signing identity to keep a stable local app identity.",
    "",
  ].join("\n"));
}

if (isMac && !environment.APPLE_SIGNING_IDENTITY) {
  const identity = findAppleDevelopmentIdentity();
  if (identity) {
    environment.APPLE_SIGNING_IDENTITY = identity;
    log(`Using the macOS development signing identity “${identity}”.`);
  } else {
    environment.APPLE_SIGNING_IDENTITY = "-";
    usingAdHocSignature = true;
    console.warn("No Apple Development signing identity was found. Using an ad-hoc signature.");
  }
}
usingAdHocSignature = isMac && environment.APPLE_SIGNING_IDENTITY === "-";

if (isMac) {
  detachMountedBuildImages();
}

if (isWindows) {
  try {
    stopRunningWindowsBuild();
  } catch (error) {
    console.error(`Could not stop the running Windows build: ${error.message}`);
    process.exit(1);
  }
}

const npmCli = npmCliPath();
if (!npmCli) {
  console.error(
    "Could not locate npm's CLI. Run this build through `npm run build`, or reinstall Node.js with npm included.",
  );
  process.exit(1);
}

const args = ["run", "tauri:build", "--workspace", "@dbm/desktop"];
const tauriArgs = process.argv.slice(2);
const hasBundleOverride = tauriArgs.some(
  (argument) =>
    argument === "--bundles" ||
    argument === "-b" ||
    argument.startsWith("--bundles=") ||
    argument.startsWith("-b="),
);
if (!hasBundleOverride) {
  if (isMac) {
    tauriArgs.unshift("--bundles", "app,dmg");
  } else if (isWindows) {
    tauriArgs.unshift("--bundles", "nsis");
  } else if (process.platform === "linux") {
    tauriArgs.unshift("--bundles", "deb,appimage");
  }
}
if (tauriArgs.length > 0) {
  args.push("--", ...tauriArgs);
}

const result = spawnSync(process.execPath, [npmCli, ...args], {
  env: environment,
  stdio: "inherit",
  cwd: ROOT,
});

if (result.error) {
  console.error(`Failed to start the desktop build through npm: ${result.error.message}`);
  process.exit(1);
}
if ((result.status ?? 1) !== 0) {
  process.exit(result.status ?? 1);
}

printBuildOutputs();

if (isMac && !skipInstall) {
  try {
    if (usingAdHocSignature) {
      printAdHocKeychainWarning();
    }
    quitRunningDbm();
    installToApplications();
    if (openAfterInstall) {
      log(`Launching ${APP_NAME}…`);
      runChecked("/usr/bin/open", [APPLICATIONS_APP], { stdio: "inherit" });
    } else {
      log(`Launch with: open -a ${APP_NAME}`);
    }
  } catch (error) {
    console.error(`Build succeeded, but install failed: ${error.message}`);
    process.exit(1);
  }
} else if (isMac) {
  log("Skipping Applications install (DBM_SKIP_INSTALL=1).");
  log(`Run this build with: open "${BUILT_APP}"`);
}

process.exit(0);
