# Rex's icon

On Windows, Rexy can show Rex's own app icon on its window, taskbar button, toasts, program file and installer, in place of its own drawing (`assets/dev.rexy.client.svg`). The icon is Superlogical's artwork, taken from the Mac's Rex.app, and **is not kept in this repository**.

To use it, put it here as `rex.png`: 256 pixels square, RGBA. On a Mac with Rex installed, the largest size in the app's `.icns` will do:

```sh
sips -s format png -z 256 256 /Applications/Rex.app/Contents/Resources/*.icns --out rex.png
```

Whichever is here when Rexy is built goes into it, and `scripts/package.ps1` makes `assets/rexy.ico` from it, or from Rexy's drawing when it is missing. Before passing on a build that has it, ask Superlogical.
