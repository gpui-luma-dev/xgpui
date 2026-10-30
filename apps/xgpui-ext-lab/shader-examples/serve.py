#!/usr/bin/env python3
"""Serve the built demos on localhost, including the WebAssembly MIME type."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm"}

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8083)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parents[3] / "target" / "web" / "shaders"
    if not (directory / "shader_examples_bg.wasm").is_file():
        parser.error("build the demos first: bash apps/xgpui-ext-lab/shader-examples/build.sh")
    server = ThreadingHTTPServer(
        ("127.0.0.1", args.port), partial(Handler, directory=str(directory))
    )
    print(f"Instanced shader examples: http://localhost:{args.port}/", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
