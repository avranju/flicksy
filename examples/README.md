# Small, self-contained example

Generate a 12-second synthetic tone and two gradient images from the project
root. This requires FFmpeg and ImageMagick 7 (`magick`):

```sh
mkdir -p examples/media
ffmpeg -hide_banner -nostdin -y \
  -f lavfi -i 'sine=frequency=440:duration=12' \
  -af 'volume=0.1' -c:a pcm_s16le examples/media/tone.wav
magick -size 1280x720 'gradient:#6554e8-#111827' examples/media/purple.png
magick -size 1280x720 'gradient:#e8b454-#172554' examples/media/gold.png
```

In Flicksy:

1. Set the title to **Example video**.
2. Upload `examples/media/tone.wav` as the song.
3. Upload `purple.png` and `gold.png` in that order as the images.
4. Optionally add an opening credit such as **Created with Flicksy**.
5. Set the additional instructions to **Make a 12-second landscape video with
   gentle cross-fades.**
6. Start production, then preview or download the result.

The inputs are generated locally without third-party recordings or photographs;
you may freely use this example material. These are test inputs, not a promise of
a specific render: output depends on your model and installed runtime tools.
The generated `examples/media/` directory is excluded from Git and the Docker
build context. Remove it when you no longer need it. The generation commands use
`-y` and will overwrite existing files with the same names in that directory.
