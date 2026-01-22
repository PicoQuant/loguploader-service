import argparse
import os
import sys
import traceback

import nextcloud_client


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--link",
        default=os.environ.get("PUBLIC_LINK"),
        help="Nextcloud public link (or set env var PUBLIC_LINK)",
    )
    parser.add_argument(
        "--file",
        required=True,
        help="Path to a local file to upload (e.g. a .zip)",
    )
    args = parser.parse_args()

    if not args.link:
        print("Missing --link (or set PUBLIC_LINK env var).", file=sys.stderr)
        return 2

    file_path = os.path.abspath(args.file)
    if not os.path.isfile(file_path):
        print(f"File not found: {file_path}", file=sys.stderr)
        return 2

    print(f"Using file: {file_path}")
    print(f"Using link: {args.link}")

    try:
        nc = nextcloud_client.Client.from_public_link(args.link)
        print(f"Client created: {nc!r}")
    except Exception as e:
        print(f"Failed to create client: {type(e).__name__}: {e}", file=sys.stderr)
        traceback.print_exc()
        return 1

    try:
        result = nc.drop_file(file_path)
        print(f"drop_file returned: {result!r}")
        if result:
            print("Upload OK")
            return 0
        print("Upload FAILED (drop_file returned falsy)")
        return 1
    except Exception as e:
        print(f"Upload raised exception: {type(e).__name__}: {e}", file=sys.stderr)
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    raise SystemExit(main())