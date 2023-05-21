#!/bin/bash

VERSION="$1"

sed -i '/\[package\]/,/^version = "[^"]*"$/ s/^version = "[^"]*"$/version = "'"$VERSION"'"/' Cargo.toml
