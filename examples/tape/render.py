import pty
import os
import subprocess
import time
import json
import select
import re
import sys
import tempfile
import concurrent.futures

# --- CONFIGURATION ---
TYPE_SPEED = 0.1
WAIT_AFTER = 0.5


def record(output_cast, scenario_file, common_file="common.exp", width=80, height=24):
    name = os.path.basename(scenario_file).replace(".exp", "")
    tape_dir = os.getcwd()
    lade_bin = os.path.abspath(os.path.join(tape_dir, "../../target/debug/lade"))

    with tempfile.TemporaryDirectory(prefix=f"lade-tape-{name}-") as home_dir:
        env = {
            "HOME": home_dir,
            "ZDOTDIR": home_dir,
            "TERM": "xterm-256color",
            "PATH": os.path.dirname(lade_bin) + ":" + os.environ.get("PATH", ""),
            "VAULT_ADDR": "http://127.0.0.1:8200",
            "VAULT_TOKEN": "token",
            "LADE_VAULT_HTTP": "1",
            "LADE_CONFIG_PATH": os.path.join(home_dir, ".lade-test-config.json"),
            "LADE_SHELL": "zsh",
            "USER": "bob",
            "USERNAME": "bob",
        }

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
        pid = os.fork()

        if pid == 0:
            os.close(fd)
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

        # 1. Setup (silent)
        time.sleep(1.0)
        for cmd in setup_commands:
            os.write(fd, (cmd + "\r").encode())
            time.sleep(0.1)

        # Clear everything
        os.write(fd, b"unset LADE_NOT_FIRST; clear\r")
        time.sleep(0.5)

        # Flush ALL initial output
        while True:
            r, _, _ = select.select([fd], [], [], 0.2)
            if r:
                os.read(fd, 8192)
            else:
                break

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
        except:
            pass


def sanitize_text(text):
    text = re.sub(r"(?:\x1B[@-_][0-?]*[ -/]*[@-~])", "", text)
    chars = []
    for char in text:
        if char == "\b":
            if chars:
                chars.pop()
        else:
            chars.append(char)
    text = "".join(chars)
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = text.replace("[?2004h", "").replace("[?2004l", "")
    text = re.sub(r"\n{3,}", "\n\n", text)
    text = text.lstrip("\n")
    text = re.sub(r"(> .*\n)\n([^>\n])", r"\1\2", text)
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

        # Parallel execution using ProcessPoolExecutor
        # ProcessPool is better for CPU-bound tasks like agg rendering
        with concurrent.futures.ProcessPoolExecutor() as executor:
            executor.map(generate_outputs, tapes)
