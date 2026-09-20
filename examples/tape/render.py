import pty
import os
import subprocess
import time
import json
import select
import re
import sys
import tempfile
import fcntl
import struct
import termios
import shutil

# --- CONFIGURATION ---
TYPE_SPEED = 0.1
WAIT_AFTER = 0.5


def record(output_cast, scenario_file, common_file="common.exp", width=80, height=24):
    name = os.path.basename(scenario_file).replace(".exp", "")
    tape_dir = os.getcwd()
    repo_root = os.path.abspath(os.path.join(tape_dir, "../.."))
    target_dir = os.environ.get("CARGO_TARGET_DIR", os.path.join(repo_root, "target"))
    if not os.path.isabs(target_dir):
        target_dir = os.path.join(repo_root, target_dir)
    release_bin = os.path.join(target_dir, "release", "lade")
    debug_bin = os.path.join(target_dir, "debug", "lade")
    lade_bin = release_bin if os.path.exists(release_bin) else debug_bin
    host_kubeconfig = os.environ.get("KUBECONFIG", os.path.expanduser("~/.kube/config"))

    with tempfile.TemporaryDirectory(prefix=f"lade-tape-{name}-") as home_dir:
        stub_dir = seed_mise_stub(home_dir)
        env = {
            "HOME": home_dir,
            "ZDOTDIR": home_dir,
            "TERM": "xterm-256color",
            "PATH": stub_dir
            + ":"
            + os.path.dirname(lade_bin)
            + ":"
            + os.environ.get("PATH", ""),
            "KUBECONFIG": host_kubeconfig,
            "VAULT_ADDR": "http://127.0.0.1:8200",
            "VAULT_TOKEN": "token",
            "LADE_VAULT_HTTP": "1",
            "LADE_CONFIG_PATH": os.path.join(home_dir, ".lade-test-config.json"),
            "LADE_SHELL": "zsh",
            "USER": "bob",
            "USERNAME": "bob",
            "MISE_DATA_DIR": os.path.join(home_dir, ".local/share/mise"),
            "MISE_INSTALLS_DIR": os.path.join(home_dir, ".local/share/mise/installs"),
        }
        pins = tape_pins(tape_dir)
        seed_kubectl_store(home_dir, tape_dir, pins["kubectl"])
        seed_vault_store(home_dir, pins["vault"])
        work_dir = seed_work_dir(home_dir, tape_dir, pins)

        with open(os.path.join(home_dir, ".zshrc"), "w") as f:
            f.write("unsetopt PROMPT_SP\n")
            f.write("PROMPT='> '\n")
            f.write("precmd_lade_tape() {\n")
            f.write("  if [[ -n $LADE_NOT_FIRST ]]; then\n")
            f.write("    print\n")
            f.write("  fi\n")
            f.write("  export LADE_NOT_FIRST=1\n")
            f.write("}\n")
            f.write("precmd_functions=(precmd_lade_tape)\n")

        setup_commands = []
        if os.path.exists(common_file):
            with open(common_file, "r") as f:
                setup_commands = [line.strip() for line in f if line.strip()]

        with open(scenario_file, "r") as f:
            all_commands = [line.strip() for line in f if line.strip()]

        commands = []
        if "clear" in all_commands:
            idx = all_commands.index("clear")
            setup_commands.extend(all_commands[: idx + 1])
            commands = all_commands[idx + 1 :]
        else:
            commands = all_commands

        fd, child_fd = pty.openpty()
        fcntl.ioctl(child_fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        pid = os.fork()

        if pid == 0:
            os.close(fd)
            os.chdir(work_dir)
            os.dup2(child_fd, 0)
            os.dup2(child_fd, 1)
            os.dup2(child_fd, 2)
            os.execvpe("zsh", ["zsh"], env)

        os.close(child_fd)

        events = []
        virtual_time = 0.0

        def log_event(text, delay=0.0):
            nonlocal virtual_time
            virtual_time += delay
            events.append([round(virtual_time, 3), "o", text])

        def drain(until_prompt=False, quiet=2.0):
            last_output = time.time()
            buf = ""
            while True:
                r, _, _ = select.select([fd], [], [], 0.3)
                if r:
                    chunk = os.read(fd, 8192)
                    if not chunk:
                        break
                    buf += chunk.decode("utf-8", errors="replace")
                    last_output = time.time()
                    if until_prompt:
                        visible = re.sub(r"(?:\x1B[@-_][0-?]*[ -/]*[@-~])", "", buf)
                        visible = visible.replace("\r", "\n")
                        if re.search(r">\s*$", visible):
                            break
                elif time.time() - last_output >= quiet:
                    break
            return buf

        # 1. Setup (silent)
        time.sleep(1.0)
        for cmd in setup_commands:
            os.write(fd, (cmd + "\r").encode())
            time.sleep(0.1)
            drain(until_prompt=True)

        # Reset screen without typing an echoed command.
        os.write(fd, b"\x1bc")
        time.sleep(0.3)
        drain(until_prompt=True, quiet=2.0)

        # 2. Start recording
        # Trigger first prompt
        os.write(fd, b"\r")

        for i, cmd in enumerate(commands):
            # Wait for ANY prompt (> or interactive)
            output_accum = ""
            while True:
                r, _, _ = select.select([fd], [], [], 1.0)
                if r:
                    res = os.read(fd, 8192).decode("utf-8", errors="replace")
                    output_accum += res
                    if ">" in res or "continue" in res or "cancel):" in res:
                        break
                else:
                    break

            if output_accum:
                # Add prompt/output with fixed delay
                log_event(output_accum, delay=0.05)

            # Type command
            for char in cmd:
                os.write(fd, char.encode())
                time.sleep(TYPE_SPEED)
                # Capture echo
                r, _, _ = select.select([fd], [], [], 0.1)
                if r:
                    echo = os.read(fd, 4096).decode("utf-8", errors="replace")
                    log_event(echo, delay=TYPE_SPEED)
                else:
                    log_event(char, delay=TYPE_SPEED)

            # Enter
            os.write(fd, b"\r")

            # Read result until next prompt
            output_accum = ""
            while True:
                r, _, _ = select.select([fd], [], [], 1.0)
                if r:
                    res = os.read(fd, 8192).decode("utf-8", errors="replace")
                    output_accum += res
                    if (
                        'Type "yes" to continue' in res
                        or "cancel):" in res
                        or res.strip().endswith(">")
                    ):
                        break
                else:
                    break

            if output_accum:
                log_event(output_accum, delay=0.05)

            time.sleep(WAIT_AFTER)

        # Final pause
        log_event("", delay=2.0)
        os.write(fd, b"exit\r")

        # Save .cast
        header = {
            "version": 2,
            "width": width,
            "height": height,
            "timestamp": 1589454000,
            "env": {"TERM": "xterm-256color", "SHELL": "/bin/zsh"},
        }

        # Post-process to ensure we start at the first prompt
        processed_events = []
        found_prompt = False
        start_vtime = 0.0
        for e in events:
            if not found_prompt:
                if ">" in e[2]:
                    found_prompt = True
                    idx = e[2].find(">")
                    e[2] = e[2][idx:].lstrip("\r\n")
                    if e[2]:
                        start_vtime = e[0]
                        e[0] = 0.0
                        processed_events.append(e)
            else:
                e[0] = round(e[0] - start_vtime, 3)
                processed_events.append(e)

        if not processed_events:
            processed_events = events

        with open(output_cast, "w") as f:
            f.write(json.dumps(header) + "\n")
            for e in processed_events:
                f.write(json.dumps(e) + "\n")

        os.close(fd)
        try:
            os.waitpid(pid, 0)
        except OSError:
            pass


def seed_work_dir(home_dir, tape_dir, pins):
    work_dir = "/tmp/lade-tape"
    if os.path.exists(work_dir):
        shutil.rmtree(work_dir)
    os.makedirs(work_dir, exist_ok=True)
    for name in ("lade.yml", "show-example"):
        src = os.path.join(tape_dir, name)
        if os.path.exists(src):
            dest = os.path.join(work_dir, name)
            shutil.copy2(src, dest)
            if name == "show-example":
                os.chmod(dest, 0o755)
    write_tape_lock(work_dir, pins)
    subprocess.run(
        ["git", "init", "--quiet"],
        cwd=work_dir,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    sources_src = os.path.abspath(os.path.join(tape_dir, "..", "sources"))
    sources_dest = "/tmp/sources"
    if os.path.isdir(sources_src):
        if os.path.exists(sources_dest):
            shutil.rmtree(sources_dest)
        shutil.copytree(sources_src, sources_dest)
    return work_dir


def write_tape_lock(work_dir, pins):
    path = os.path.join(work_dir, "lade.lock")
    with open(path, "w") as f:
        f.write("lockfile_version = 1\n\n")
        f.write("[[tools.vault]]\n")
        f.write(f'version = "{pins["vault"]}"\n')
        f.write('backend = "aqua:hashicorp/vault"\n\n')
        f.write("[[tools.kubectl]]\n")
        f.write(f'version = "{pins["kubectl"]}"\n')
        f.write('backend = "aqua:kubernetes/kubectl"\n')


def seed_mise_stub(home_dir):
    stub_dir = os.path.join(home_dir, "bin")
    os.makedirs(stub_dir, exist_ok=True)
    dest = os.path.join(stub_dir, "mise")
    with open(dest, "w") as f:
        f.write("#!/bin/sh\n")
        f.write('if [ "$1" = "--version" ]; then\n')
        f.write("  echo 2025.8.11\n")
        f.write("  exit 0\n")
        f.write("fi\n")
        f.write("exit 0\n")
    os.chmod(dest, 0o755)
    return stub_dir


def tape_pins(tape_dir):
    path = os.path.join(tape_dir, "lade.yml")
    with open(path, "r") as f:
        text = f.read()
    pins = {}
    for key in ("vault", "kubectl"):
        match = re.search(
            rf'(?:^|\n)\s*{re.escape(key)}:\s*["\']?mise://[^\s"\']+@([^\s"\']+)',
            text,
        )
        if not match:
            raise SystemExit(f"lade.yml is missing a mise pin for {key}")
        pins[key] = match.group(1)
    return pins


def seed_vault_store(home_dir, version):
    dest_dir = os.path.join(
        home_dir, ".local", "share", "mise", "installs", "vault", version
    )
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, "vault")
    with open(dest, "w") as f:
        f.write("#!/bin/sh\nexit 0\n")
    os.chmod(dest, 0o755)


def seed_kubectl_store(home_dir, tape_dir, version):
    kubectl = shutil.which("kubectl")
    if not kubectl:
        return
    dest_dir = os.path.join(
        home_dir, ".local", "share", "mise", "installs", "kubectl", version
    )
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, "kubectl")
    shutil.copy2(kubectl, dest)
    os.chmod(dest, 0o755)
    lock = os.path.join(tape_dir, "lade.lock")
    if not os.path.exists(lock):
        with open(lock, "w") as f:
            f.write(
                "lockfile_version = 1\n\n"
                "[[tools.kubectl]]\n"
                f'version = "{version}"\n'
                'backend = "aqua:kubernetes/kubectl"\n'
            )


def scrub_tape_paths(text):
    text = text.replace("/private/tmp/lade-tape", ".")
    text = text.replace("/tmp/lade-tape", ".")
    text = re.sub(r"/private/var/folders/\S+/lade-tape-\S+", ".", text)
    return re.sub(r"/var/folders/\S+/lade-tape-\S+", ".", text)


def scrub_cast(cast_file):
    with open(cast_file, "r") as f:
        lines = f.readlines()
    out = [lines[0]]
    for line in lines[1:]:
        event = json.loads(line)
        if event[1] == "o":
            event[2] = scrub_tape_paths(event[2])
        out.append(json.dumps(event) + "\n")
    with open(cast_file, "w") as f:
        f.writelines(out)


def sanitize_text(text):
    text = re.sub(r"(?:\x1B[@-_][0-?]*[ -/]*[@-~])", "", text)
    text = text.replace("\r\n", "\n")
    chars = []
    for char in text:
        if char == "\b":
            if chars:
                chars.pop()
        else:
            chars.append(char)
    text = "".join(chars)
    text = text.replace("\r", "\n")
    text = text.replace("[?2004h", "").replace("[?2004l", "")
    text = scrub_tape_paths(text)
    text = re.sub(r"^unset LADE_NOT_FIRST; clear\n?", "", text, flags=re.M)
    text = re.sub(r"\n{3,}", "\n\n", text)
    text = text.lstrip("\n")
    text = re.sub(r"(> .*\n)\n([^>\n])", r"\1\2", text)
    lines = []
    progress = set()
    continuations = set()
    in_progress = False
    continuation_kind = ""
    for line in text.splitlines():
        if line.startswith("> "):
            progress = set()
            continuations = set()
            in_progress = False
            continuation_kind = ""
        if line.startswith(("⠋", "⠙", "⠸", "⠴", "⠦", "⠇", "✔", "✘")):
            in_progress = True
            if line.startswith("✔"):
                kind = "success"
            elif line.startswith("✘"):
                kind = "failed"
            else:
                kind = "loading"
            continuation_kind = kind
            resource = re.sub(r"^[^ ]+ ", "", line)
            resource = re.sub(r"pid=\d+ \d+ ms|\b\d+ ms\b", "", resource).strip()
            key = (kind, resource)
            if key in progress:
                continue
            progress.add(key)
        elif in_progress and line:
            continuation = re.sub(r"pid=\d+ \d+ ms|\b\d+ ms\b", "", line).strip()
            key = (continuation_kind, continuation)
            if key in continuations:
                continue
            continuations.add(key)
        lines.append(line)
    text = "\n".join(lines)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.rstrip() + "\n"


def generate_outputs(name):
    exp_file = f"{name}.exp"
    cast_file = f"{name}.cast"
    gif_file = f"{name}.gif"
    txt_file = f"{name}.txt"

    if not os.path.exists(exp_file):
        return

    # Original VHS was 640x320 (2:1 ratio). Retina x2 is 1280x640.
    # To get ~2:1 ratio with Menlo (0.6 width) and 1.2 line height:
    # Ratio = (cols * 0.6) / (rows * 1.2) = cols / (2 * rows)
    # For 2:1, cols = 4 * rows.
    # 80 cols -> 20 rows. 83 cols -> 21 rows.
    width, height = 80, 20
    if name == "main":
        width, height = 83, 21

    target_width = 1280 if width == 80 else 1328
    target_height = 640

    print(f"Recording {name}...")
    record(cast_file, exp_file, width=width, height=height)
    scrub_cast(cast_file)

    full_text = ""
    with open(cast_file, "r") as f:
        lines = f.readlines()
        for line in lines[1:]:
            event = json.loads(line)
            if event[1] == "o":
                full_text += event[2]

    clean_text = sanitize_text(full_text)
    with open(txt_file, "w") as f:
        f.write(clean_text)

    print(f"Generating GIF {gif_file}...")
    tmp_gif = f"{name}.tmp.gif"
    if os.path.exists(tmp_gif):
        os.remove(tmp_gif)

    # Use 32px font for Retina quality
    subprocess.run(
        [
            "agg",
            "--theme",
            "solarized-light",
            "--font-family",
            "Menlo",
            "--font-size",
            "32",
            "--line-height",
            "1.2",
            "--renderer",
            "resvg",
            cast_file,
            tmp_gif,
        ],
        check=True,
    )

    # Scale to exact 2x dimensions while preserving aspect ratio (padding if necessary)
    # to avoid the "squashed" look.
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-i",
            tmp_gif,
            "-vf",
            f"scale={target_width}:{target_height}:force_original_aspect_ratio=decrease,pad={target_width}:{target_height}:(ow-iw)/2:(oh-ih)/2:color=#FDF6E3,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse=dither=none",
            gif_file,
        ],
        check=True,
    )

    if os.path.exists(tmp_gif):
        os.remove(tmp_gif)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        generate_outputs(sys.argv[1])
    else:
        # Collect all tapes to render
        tapes = [
            f[:-4] for f in os.listdir(".") if f.endswith(".exp") and f != "common.exp"
        ]

        for tape in sorted(tapes):
            generate_outputs(tape)
