/* Included before every source of nvi2 (build.sh): what libbsd gives nvi2 on Linux. */
#ifndef ZORAYA_NVI_COMPAT_H
#define ZORAYA_NVI_COMPAT_H
const char *getprogname(void);
void zoraya_setprogname(const char *argv0);
#endif
