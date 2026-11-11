name     netd
summary  the network service: the interface, its address and the connections of programs
usage    netd
cwd      none
setting  net.address   choice dhcp|static  dhcp  how the interface gets its address
setting  net.ip        prefix ''  the address and prefix length with static, a.b.c.d/len
setting  net.gateway   address ''  the gateway with static
setting  net.dns       list of address 0 4  ''  DNS servers in order; empty means the ones DHCP gave
setting  net.hosts     list of text 0 64 unique  ''  local names, name=address
setting  net.hostname  text  zoraya  the name DHCP announces
---
The network service owns the interface, the address and every connection a
program makes (ADR-0221). It reads the net settings in /config/settings when
it starts; a change applies the next time it starts.

FILES
  /config/settings   settings

SEE ALSO
  net, help settings
