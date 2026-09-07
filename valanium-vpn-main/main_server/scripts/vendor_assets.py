"""
Downloads Bootstrap/Bootstrap Icons once into main_server/static/vendor/ so the
admin panel no longer depends on cdn.jsdelivr.net at every page load (which is
slow/unreliable from restricted networks and adds latency on weak hardware).

Run once, wherever you have internet access:
    python scripts/vendor_assets.py
"""
import os
import urllib.request

BASE = os.path.join(os.path.dirname(__file__), "..", "static", "vendor")

FILES = {
    "css/bootstrap.min.css": "https://cdn.jsdelivr.net/npm/bootstrap@5.3.2/dist/css/bootstrap.min.css",
    "js/bootstrap.bundle.min.js": "https://cdn.jsdelivr.net/npm/bootstrap@5.3.2/dist/js/bootstrap.bundle.min.js",
    "css/bootstrap-icons.css": "https://cdn.jsdelivr.net/npm/bootstrap-icons@1.11.1/font/bootstrap-icons.css",
    "css/fonts/bootstrap-icons.woff2": "https://cdn.jsdelivr.net/npm/bootstrap-icons@1.11.1/font/fonts/bootstrap-icons.woff2",
    "css/fonts/bootstrap-icons.woff": "https://cdn.jsdelivr.net/npm/bootstrap-icons@1.11.1/font/fonts/bootstrap-icons.woff",
}


def main():
    for rel_path, url in FILES.items():
        dest = os.path.join(BASE, rel_path)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        print(f"Fetching {url} -> {dest}")
        urllib.request.urlretrieve(url, dest)
    print("Done. The admin panel now serves these assets locally from /static/vendor/.")


if __name__ == "__main__":
    main()
