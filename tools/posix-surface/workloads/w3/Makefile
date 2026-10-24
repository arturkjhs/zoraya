# W3-j (TASK-02b): the TASK-02 W3 SQLite build (same commands as remote/workloads.sh: W3) as a Makefile, so make -j
# can run the two compiles concurrently.
all: sqlite3

sqlite3: shell.o sqlite3.o
	tcc -o $@ shell.o sqlite3.o -lm

sqlite3.o: sqlite3.c
	tcc -O2 -DSQLITE_THREADSAFE=0 -c sqlite3.c

shell.o: shell.c
	tcc -c shell.c
