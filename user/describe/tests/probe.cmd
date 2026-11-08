name     probe
summary  say what a command got, for the grant tests
usage    probe [--read FILE] [--change FILE] [--create FILE] [--remove FILE] [--tree DIR] [PATH...]
option   --read FILE read  a file to be given to read
option   --change FILE write  a file to be given to change
option   --create FILE create  a name to be given to create or replace
option   --remove FILE remove  a name to be given to remove
option   --tree DIR tree  a directory to be given whole
operand  [PATH...]  text  paths to open through its namespace, to read and to change
exit     0  it said what it got
---
Prints the type of the handle at each of its slots, then each entry of its start-up record with
what every file request gives through it (stat, widening the rights, and for a directory list,
create, mkdir, remove and sync; for a file read, write and sync), then what opening each PATH
gives, twice. For the tests of ADR-0153 (INV-0153-1, INV-0153-2, INV-0153-7).
