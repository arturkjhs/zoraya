# TASK-27 9c, ADR-0320: a blocking HTTPS client over the standard library's
# http.client or urllib, the path that goes through the file-descriptor BIO of
# patch 0001 (asyncio uses a memory BIO). argv: HOST PORT http.client|urllib.
# The context has no cafile: the roots are OpenSSL's default (/system/tls/roots).
# A refused certificate ends before any request byte (INV-0320-3).
import faulthandler, http.client, ssl, sys, urllib.error, urllib.request

faulthandler.dump_traceback_later(40, exit=True)

host, port, how = sys.argv[1], int(sys.argv[2]), sys.argv[3]
ctx = ssl.create_default_context()
# The test PKI has no Authority Key Identifier (see https_client.py).
ctx.verify_flags &= ~ssl.VERIFY_X509_STRICT

try:
    if how == "http.client":
        c = http.client.HTTPSConnection(host, port, context=ctx, timeout=20)
        c.request("GET", "/")
        r = c.getresponse()
        status, body = r.status, r.read()
        c.close()
    else:
        with urllib.request.urlopen("https://%s:%d/" % (host, port), context=ctx, timeout=20) as r:
            status, body = r.status, r.read()
except ssl.SSLCertVerificationError as e:
    print("refused", e.verify_message)
    sys.exit(3)
except urllib.error.URLError as e:
    if isinstance(e.reason, ssl.SSLCertVerificationError):
        print("refused", e.reason.verify_message)
        sys.exit(3)
    raise
print("status", status)
print("body", body.decode().strip().replace("\n", " | "))
