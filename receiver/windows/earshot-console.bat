@echo off
rem Earshot in a console window, with live statistics.
rem
rem You probably want Earshot.exe instead - it sits in the notification area
rem with no window at all. This one is for when something is wrong and you
rem want to watch the numbers: packets a second, milliseconds buffered, and
rem how much was lost.
rem
rem Both need a virtual audio cable. See "START HERE.txt".
title Earshot
"%~dp0earshot-receiver.exe" --virtual-mic
