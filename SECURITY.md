# Security

## Reporting a vulnerability

Please report security problems privately through GitHub: on the repository's **Security** tab, choose **Report a vulnerability**. Don't open a public issue.

Include what you found, how to reproduce it (a sample `.vde` project or media file helps), and the videode version or commit and operating system.

You'll get an acknowledgement within a week. Once a fix is ready, it ships in a release and you're credited in the advisory unless you'd rather not be.

## Scope

videode opens files that may come from anyone, so the most likely problems are in reading them:

- `.vde` project files (KDL), including paths they point at
- media files, which are decoded by FFmpeg
- the demo footage generator, which runs the `ffmpeg` command-line tool

Bugs in FFmpeg itself should go to the [FFmpeg project](https://ffmpeg.org/security.html); tell us too if videode makes them easier to reach.

## Supported versions

videode is pre-1.0. Only the latest commit on `main` gets security fixes.
