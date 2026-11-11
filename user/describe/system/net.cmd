name     net
summary  show the state of the network
usage    net
usage    net status
operand  [WORD]  text  status, the only thing net shows
cwd      none
exit     0  shown
exit     1  the network is not available
---
Prints the interface, its address and where the address came from, the
gateway, the DNS servers and the frame counters. The address is the lease the
network service holds; it changes with a renewal, and nothing writes it to
/config. To change the configuration, use settings net.NAME VALUE.

EXAMPLES
  net status

SEE ALSO
  settings
