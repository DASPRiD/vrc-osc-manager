#!/bin/bash

inkscape --actions="export-area:0:0:64:64; export-filename:assets/icon-dark-inactive.png; export-do" assets/icons.svg
inkscape --actions="export-area:64:0:128:64; export-filename:assets/icon-dark-active.png; export-do" assets/icons.svg
inkscape --actions="export-area:0:64:64:128; export-filename:assets/icon-light-inactive.png; export-do" assets/icons.svg
inkscape --actions="export-area:64:64:128:128; export-filename:assets/icon-light-active.png; export-do" assets/icons.svg

convert assets/icon-dark-inactive.png assets/icon-dark-inactive.ico
convert assets/icon-dark-active.png assets/icon-dark-active.ico
convert assets/icon-light-inactive.png assets/icon-light-inactive.ico
convert assets/icon-light-active.png assets/icon-light-active.ico

