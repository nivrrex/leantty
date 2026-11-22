@echo off
echo ran>C:\dev\tty\vt-verify-win\glyph-run\marker.txt
chcp 65001>nul
type "C:\dev\tty\vt-verify-win\glyph-run\corpus.txt"
ping -n 600 127.0.0.1>nul
