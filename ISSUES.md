# Open issues

## 1. Workmux Linux startup fails when the shared Aven directory is missing

- **Status:** Open.
- **Configuration:** `~/.config/workmux/config.yaml` mounts `{project_root}/.aven` at `{container_home}/.aven`.
- **Reproduction:** With no `.aven` directory in the project root, run `workmux add --config ~/.config/workmux/agents.linux.yaml --background pi-linux-smoke --base main`.
- **Observed:** Workmux creates the worktree and terminal, but the sandbox launch stops before Pi starts:
  ```text
  Error: extra_mounts: expanded host path does not exist: /Users/kenbanks/Software/DevBox/.aven
  ```
- **Expected:** The default Linux workflow should provision its required shared directory, or clearly explain the required initialization before creating a failed worker.
- **Verification:** Temporarily creating the empty directory allowed startup to proceed to issue 2. The directory was removed during cleanup; this issue remains unresolved.

## 2. Workmux Linux launch rejects Pi credentials although the direct Linux launch works

- **Status:** Open; root cause not yet established.
- **Reproduction:** Ensure the shared `.aven` mount source exists, then launch a worker using `~/.config/workmux/agents.linux.yaml`.
- **Observed:** Pi exits before accepting a prompt:
  ```text
  pi: Pi auth.json contains unmigrated credentials. Run tools/pi-auth-import before granting the profile to nono.
  ```
- **Comparison:** The user verified that launching `linux`, running `pi`, and entering `hi` successfully returned a model response. The equivalent Workmux path failed before a prompt could be submitted.
- **Relevant configuration:** Workmux uses `sandbox.agent_config_dir: ~/.config/linux/pi/agent`, image `debian-dev:latest`, and container home `/home/dev`.
- **Error source:** `resources/scripts/agent-launch.cjs`, in `assertPhantomAuth()`. It requires supported OAuth entries whose placeholder tokens have matching mappings in nono's persistent store. This error alone does not distinguish real unmigrated credentials from missing matching state or unsupported entries.
- **Expected:** Workmux should provide the same working model authentication as the direct Linux launcher, without manual credential edits or token replacement.
- **Next investigation:** Compare the Pi profile, nono state/config mounts, and environment used by Workmux against the working direct launcher. Do not assume credential migration is needed merely from this error.
- **Migration usability gap:** The error references `tools/pi-auth-import`, a Rust source directory; the current container build does not install that helper as an executable.
- **Verification:** No credentials were changed or imported. The temporary smoke-test worktree and branch were removed. This issue remains unresolved.
