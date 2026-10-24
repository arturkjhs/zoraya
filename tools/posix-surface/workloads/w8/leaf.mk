# W8 leaf rules, included by every leaf Makefile (a/1, a/2, b/1, b/2). Explicit rules only, so GNU make and
# pdpmake read it the same way.
TOP = ../..
OBJS = m1.o m2.o m3.o m4.o m5.o m6.o m7.o m8.o

all: libleaf.a

libleaf.a: $(OBJS)
	tcc -ar rcs $@ $(OBJS)

mktab: $(TOP)/mktab.c
	tcc -o $@ $(TOP)/mktab.c

t1.h: mktab
	./mktab 1 > $@

m1.o: $(TOP)/src/m1.c t1.h
	tcc -I. -c $(TOP)/src/m1.c -o $@

t2.h: mktab
	./mktab 2 > $@

m2.o: $(TOP)/src/m2.c t2.h
	tcc -I. -c $(TOP)/src/m2.c -o $@

t3.h: mktab
	./mktab 3 > $@

m3.o: $(TOP)/src/m3.c t3.h
	tcc -I. -c $(TOP)/src/m3.c -o $@

t4.h: mktab
	./mktab 4 > $@

m4.o: $(TOP)/src/m4.c t4.h
	tcc -I. -c $(TOP)/src/m4.c -o $@

t5.h: mktab
	./mktab 5 > $@

m5.o: $(TOP)/src/m5.c t5.h
	tcc -I. -c $(TOP)/src/m5.c -o $@

t6.h: mktab
	./mktab 6 > $@

m6.o: $(TOP)/src/m6.c t6.h
	tcc -I. -c $(TOP)/src/m6.c -o $@

t7.h: mktab
	./mktab 7 > $@

m7.o: $(TOP)/src/m7.c t7.h
	tcc -I. -c $(TOP)/src/m7.c -o $@

t8.h: mktab
	./mktab 8 > $@

m8.o: $(TOP)/src/m8.c t8.h
	tcc -I. -c $(TOP)/src/m8.c -o $@
