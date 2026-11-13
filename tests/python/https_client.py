# TASK-27 9c, ADR-0320: an asyncio HTTPS client. argv: HOST PORT. The system's
# roots (/system/tls, given with the network) are the only trust; a refused
# certificate ends before any request byte (INV-0320-3). argv: HOST PORT [default]:
# with `default` the context is create_default_context() with no cafile, whose
# roots are OpenSSL's default file (/system/tls/roots, patch 0003 of OpenSSL).
import asyncio, faulthandler, ssl, sys

# A hang is a failure with a traceback of every thread, not a wait for the runner's timeout.
faulthandler.dump_traceback_later(40, exit=True)

host, port = sys.argv[1], int(sys.argv[2])
default = len(sys.argv) > 3 and sys.argv[3] == "default"

async def get():
    ctx = ssl.create_default_context() if default else ssl.create_default_context(cafile="/system/tls/roots")
    # Python 3.13 and later set VERIFY_X509_STRICT by default, which refuses a
    # certificate without an Authority Key Identifier: the test PKI's (tests/net/tls,
    # shared with the other steps) has none. Only that flag is cleared; the chain,
    # the dates and the name are still checked, and the refusals below show it.
    ctx.verify_flags &= ~ssl.VERIFY_X509_STRICT
    try:
        r, w = await asyncio.open_connection(host, port, ssl=ctx, server_hostname=host)
    except ssl.SSLCertVerificationError as e:
        print("refused", e.verify_message)
        return 3
    w.write(b"GET / HTTP/1.0\r\nHost: " + host.encode() + b"\r\n\r\n")
    await w.drain()
    body = await r.read()
    w.close()
    print("closing", flush=True)
    try:
        await asyncio.wait_for(w.wait_closed(), 10)
    except Exception as e:
        print("wait_closed", type(e).__name__, flush=True)
    print("status", body.split(b"\r\n", 1)[0].decode())
    print("body", body.split(b"\r\n\r\n", 1)[1].decode().strip().replace("\n", " | "))
    return 0

rc = asyncio.run(get())
print("loop done", flush=True)
sys.exit(rc)
