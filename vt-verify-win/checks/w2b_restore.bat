@echo off
route add 0.0.0.0 mask 0.0.0.0 172.16.0.1 metric 0 if 10
