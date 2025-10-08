#!/bin/bash

cd rapx
cargo fmt -q
cd ..

set -e

cargo install --offline --path rapx

cargo rapx -help
