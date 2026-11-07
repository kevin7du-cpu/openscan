"""Use the already-built module from Python; no scanner code is rebuilt."""
import json
import subprocess
from pathlib import Path


class OpenScan:
    def __init__(self, executable):
        self.executable = str(Path(executable).resolve(strict=True))

    def call(self, command, request=None, *, interactive=False):
        args = [self.executable, command]
        if command == "dialog":
            args.append("--return-json")
        completed = subprocess.run(
            args, input=json.dumps(request) if request is not None else "",
            capture_output=True, text=True, encoding="utf-8",
            timeout=None if interactive else 900,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            check=False,
        )
        result = json.loads(completed.stdout)
        if result.get("protocolVersion") != 1:
            raise RuntimeError("Unsupported scanner protocol")
        if completed.returncode or not result.get("ok"):
            raise RuntimeError(result)
        return result


if __name__ == "__main__":
    import sys
    scanner = OpenScan(sys.argv[1])
    print(json.dumps(scanner.call("devices"), indent=2))
    # To let the user scan, review pages and save a PDF:
    # result = scanner.call("dialog", interactive=True)
    # if not result["cancelled"]:
    #     open_in_your_application(result["path"])
