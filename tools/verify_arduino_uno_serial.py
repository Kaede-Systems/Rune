import argparse
import sys
import time

import serial


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify Rune Arduino Uno serial output."
    )
    parser.add_argument("--port", default="COM5", help="Serial port, default: COM5")
    parser.add_argument("--baud", type=int, default=115200, help="Baud rate, default: 115200")
    parser.add_argument(
        "--expect",
        default="Hello from Rune on Arduino Uno!",
        help="Expected serial text fragment",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=8.0,
        help="Maximum time to wait in seconds, default: 8",
    )
    args = parser.parse_args()

    deadline = time.time() + args.timeout
    captured = ""

    try:
        with serial.Serial(args.port, args.baud, timeout=0.25) as ser:
            # Opening the port commonly resets the Uno; give it a moment to reboot.
            time.sleep(2.0)

            while time.time() < deadline:
                chunk = ser.read(256)
                if chunk:
                    text = chunk.decode("utf-8", errors="replace")
                    captured += text
                    sys.stdout.write(text)
                    sys.stdout.flush()
                    if args.expect in captured:
                        sys.stdout.write(
                            f"\n[ok] Found expected text on {args.port}: {args.expect}\n"
                        )
                        return 0
                else:
                    time.sleep(0.05)
    except serial.SerialException as error:
        print(f"[error] Failed to open {args.port}: {error}", file=sys.stderr)
        return 2

    print(
        f"\n[error] Timed out waiting for expected text on {args.port}: {args.expect}",
        file=sys.stderr,
    )
    if captured:
        print("[info] Captured serial output:", file=sys.stderr)
        print(captured, file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
