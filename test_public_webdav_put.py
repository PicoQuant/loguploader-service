import argparse
import os
import sys
from urllib.parse import urlparse
import requests


def share_token_from_link(link: str) -> str:
    # expects https://host/index.php/s/<TOKEN> (optionally with trailing stuff)
    path = urlparse(link).path
    parts = [p for p in path.split("/") if p]
    # find "s" and take next segment
    for i, p in enumerate(parts):
        if p == "s" and i + 1 < len(parts):
            return parts[i + 1]
    raise ValueError(f"Could not extract token from link path: {path}")


def base_url_from_link(link: str) -> str:
    u = urlparse(link)
    return f"{u.scheme}://{u.netloc}"


def _print_response(prefix: str, resp: requests.Response) -> None:
    print(prefix)
    print("Status:", resp.status_code)
    print("Response headers:", dict(resp.headers))
    body = (resp.text or "")
    print("Response body (first 1000 chars):", body[:1000])


def _try_put(url: str, local_path: str, auth_user: str, auth_pass: str) -> requests.Response:
    with open(local_path, "rb") as f:
        return requests.put(
            url,
            data=f,
            auth=(auth_user, auth_pass),
            headers={"X-Requested-With": "XMLHttpRequest"},
            timeout=60,
        )


def _try_mkcol(url: str, auth_user: str, auth_pass: str) -> requests.Response:
    return requests.request(
        "MKCOL",
        url,
        auth=(auth_user, auth_pass),
        headers={"X-Requested-With": "XMLHttpRequest"},
        timeout=60,
    )


def _is_success_status(code: int) -> bool:
    return code in (200, 201, 204)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--link", required=True, help="Public share link, e.g. https://host/index.php/s/<TOKEN>")
    ap.add_argument("--file", required=True, help="Local file path")
    ap.add_argument("--password", default=os.environ.get("SHARE_PASSWORD", ""), help="Share password if any")
    ap.add_argument("--remote-name", default=None, help="Optional remote filename override")
    ap.add_argument("--mkcol", default="inbox", help="Collection name used for MKCOL+PUT attempts")
    args = ap.parse_args()

    token = share_token_from_link(args.link)
    base = base_url_from_link(args.link)

    local_path = os.path.abspath(args.file)
    if not os.path.isfile(local_path):
        print(f"File not found: {local_path}", file=sys.stderr)
        return 2

    remote_name = args.remote_name or os.path.basename(local_path)

    attempts = []

    attempts.append(
        (
            "public.php/webdav direct",
            f"{base}/public.php/webdav/{remote_name}",
            (token, args.password),
        )
    )
    attempts.append(
        (
            "public.php/dav/files/<token> direct",
            f"{base}/public.php/dav/files/{token}/{remote_name}",
            (token, args.password),
        )
    )
    attempts.append(
        (
            "remote.php/dav/public-files direct",
            f"{base}/remote.php/dav/public-files/{token}/{remote_name}",
            (token, args.password),
        )
    )
    attempts.append(
        (
            "remote.php/dav/public-files swapped auth",
            f"{base}/remote.php/dav/public-files/{token}/{remote_name}",
            ("public", token),
        )
    )

    ok = False

    for label, url, (auth_user, auth_pass) in attempts:
        try:
            r = _try_put(url, local_path, auth_user, auth_pass)
            _print_response(f"PUT [{label}] {url}", r)
            if _is_success_status(r.status_code):
                ok = True
        except Exception as e:
            print(f"PUT [{label}] {url}")
            print(f"Exception: {type(e).__name__}: {e}")
        print("-" * 60)

    try:
        mkcol_url = f"{base}/public.php/webdav/{args.mkcol}/"
        r_mkcol = _try_mkcol(mkcol_url, token, args.password)
        _print_response(f"MKCOL [public.php/webdav] {mkcol_url}", r_mkcol)
        print("-" * 60)

        put_url = f"{base}/public.php/webdav/{args.mkcol}/{remote_name}"
        r_put = _try_put(put_url, local_path, token, args.password)
        _print_response(f"PUT [public.php/webdav MKCOL+PUT] {put_url}", r_put)
        if _is_success_status(r_put.status_code):
            ok = True
    except Exception as e:
        print(f"MKCOL+PUT Exception: {type(e).__name__}: {e}")

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())