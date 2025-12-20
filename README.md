# Pixel Shift Multi Shot

Rust tool to merge sony's Pixel Shift Multi Shot raw files into a single debayered / rgb image

It supports 4 shots and 16 shots pixel shift raw files.

## speed

Thanks to mmap and parallel processing, a 16 shot 240 megapixel pixel shift can be processed in a few seconds on a 16 core machine.

## usage

```
cargo run --release -- -o /tmp/test.ARQ -i ../images/*.ARW
```

the ARQ file can then be opened with rawtherapee etc, please refer to the reference project below

## credits

inspired by https://github.com/agriggio/make_arq
