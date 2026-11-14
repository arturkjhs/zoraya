# TASK-27 9c, ADR-0320 item 6 (patch 0002): run as `python3 < fromstdin.py`, a
# program read from standard input has no directory on sys.path, so the module
# planted in the current directory (planted.py) is not imported.
try:
    import planted
except ModuleNotFoundError:
    print("stdin program imports no planted module")
