#!/usr/bin/env cssh

head -c 8 /dev/urandom | od -An -tx1

cat /dev/urandom | head -c 8 | od -An -tx1