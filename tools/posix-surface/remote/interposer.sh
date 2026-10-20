#!/bin/sh
# Stage: generate and build the LD_PRELOAD interposer from the static symbol list, then self-test it.
. "$(dirname "$0")/common.sh"
B=$W/interposer; rm -rf "$B"; mkdir -p "$B"
python3 -I "$TOOLS/interposer/gen.py" "$RES" "$B"
cp "$TOOLS/interposer/psurf_rt.c" "$B/"
cd "$B"
gcc -O2 -fPIC -fno-builtin -ffreestanding -fno-stack-protector -U_FORTIFY_SOURCE -c psurf_rt.c psurf_names.c
gcc -c psurf_tramp.S
gcc -shared -o libpsurf.so psurf_rt.o psurf_names.o psurf_tramp.o
# the runtime may depend on nothing from libc except dlsym
und=$(nm -D --undefined-only libpsurf.so | awk '{print $NF}' | grep -v -e '^dlsym$' -e '^__cxa_finalize$' -e '^_ITM' -e '^__gmon' -e '^__deregister' -e '^__register' || true)
[ -z "$und" ] || { log "interposer has unexpected libc dependencies: $und"; exit 1; }

# self-test: variadic, vfork, setjmp, stdio through the trampolines
cat > selftest.c <<'EOF'
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <setjmp.h>
#include <sys/wait.h>
#include <string.h>
#include <stdlib.h>
static jmp_buf jb;
int main(void) {
    char b[64];
    snprintf(b, sizeof b, "%d-%s-%.2f-%ld", 7, "x", 2.5, 123456789012L);
    int fd = open("/tmp/psurf-selftest", O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0 || fcntl(fd, F_GETFD) < 0) return 2;
    dprintf(fd, "%s\n", b); close(fd);
    if (setjmp(jb) == 0) longjmp(jb, 3);
    pid_t p = vfork();
    if (p == 0) { execl("/bin/true", "true", (char *)0); _exit(9); }
    int st; waitpid(p, &st, 0);
    void *al = 0; if (posix_memalign(&al, 64, 256) || !al) return 4; free(al);
    char *m = malloc(100); strcpy(m, b); printf("%s %d\n", m, WEXITSTATUS(st)); free(m);
    return strcmp(b, "7-x-2.50-123456789012") != 0;
}
EOF
gcc -O0 -o selftest selftest.c
rm -rf /tmp/psurf-hits; mkdir -p /tmp/psurf-hits
LD_PRELOAD=$B/libpsurf.so ./selftest
got=$(cat /tmp/psurf-hits/* | cut -f3 | sort -u | tr '\n' ' ')
log "selftest hits: $got"
for s in posix_memalign snprintf open fcntl dprintf close setjmp longjmp vfork execl waitpid malloc strcpy printf free strcmp; do
  case " $got " in *" $s "*) ;; *) log "selftest: missing hit for $s"; exit 1;; esac
done
log "interposer ok: $B/libpsurf.so"
