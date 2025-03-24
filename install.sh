#!/bin/sh
cargo build
sudo cp target/debug/ff2 /usr/bin/ff
sudo chmod +x /usr/bin/ff
echo "\033[1;32mff is now installed, type \"ff\" to run!"
