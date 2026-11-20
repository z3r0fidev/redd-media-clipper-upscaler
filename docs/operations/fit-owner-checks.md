# Budgeted Reddit Video Owner Checks (Runbook)

This runbook covers the 0.5.0 (feature 005, `--fit`) check that CI cannot run: **SC-008**, a
real Reddit upload. It needs a Reddit account and a phone. Plan on about 30 minutes.

| Part | What it proves |
|---|---|
| [2](#2-make-the-files) | The packaged CLI makes the files, under their caps |
| [3](#3-upload-to-reddit) | Reddit accepts, processes and plays a `reddit-video` output and a `reddit-916` output with sound |
| [4](#4-optional-the-limits-themselves) | Optional: Reddit's 15-minute limit and the 1 GB cap |

The result decides whether the built-in `reddit-video` target can drop its *unverified* badge:
the maintainer sets `"verified": true` in `crates/rmedia-core/src/fit/builtin/reddit-video.json`
(roadmap item 3) when parts 2 and 3 pass and Reddit's Help Center still states 1 GB and 15
minutes.

Part 3 posts test videos. Use a test subreddit or your own profile, and delete the posts
afterwards if you like.

## 1. What to Test

Use the CLI from the `release.yml` artifacts for the tag `v0.5.0` (Linux
`rmedia-cli-v0.5.0-x86_64-unknown-linux-gnu.tar.gz` or Windows
`rmedia-cli-v0.5.0-x86_64-pc-windows-msvc.zip`), or a build of `main` with the bundled engine.
Check that it is 0.5.0 and that fit is available:

```bash
rmedia --version            # rmedia 0.5.0
rmedia doctor | grep -E 'video targets|fit'
# video targets: 1 (reddit-video)
```

## 2. Make the Files

Use a video of your own with sound, at least 2 minutes long and 1080p or larger (a phone or
screen recording is fine). Call it `my.mp4` below. Then make three files:

```bash
# A: a 2-minute clip for Reddit's own limits (quality mode: well under 1 GB).
rmedia clip my.mp4 -s 0 -e 2:00 --fit reddit-video -o fit-check

# B: the same clip under a tight cap (budget mode: two passes, close to 20 MB).
rmedia clip my.mp4 -s 0 -e 2:00 --fit reddit-video --fit-max-size 20MB \
  --name '{source}_tight.{ext}' -o fit-check

# C: an upright 9:16 version for Reddit's full-screen phone player.
rmedia clip my.mp4 -s 0 -e 1:00 --upscale --preset reddit-916 --fit reddit-video -o fit-check
```

Each command ends with a line such as
`✓ …/my_tight.mp4  19.6 MB of 20.0 MB (98%) · 1920x1080 · 2 passes`. Record the three lines.
Check on your computer that each file plays with sound in sync and that **B** is at most
20,000,000 bytes (`ls -l`, or the file's Properties on Windows).

## 3. Upload to Reddit

For each of **A**, **B** and **C**:

1. Create a post in a test subreddit or on your profile, choose **Images & Video**, and upload
   the file. Do not trim it in Reddit's editor.
2. Wait until Reddit has processed it and the post is live.
3. Play it in a desktop browser with sound on: it plays to the end, with the sound in sync and
   no frozen or broken frames.
4. Play it in the Reddit app on a phone: the same. For **C**, the player fills the phone
   screen upright.

Record for each file: accepted (yes/no, with any error message), processing time, plays on the
web (yes/no), plays in the app with sound (yes/no), and anything odd.

## 4. Optional: the Limits Themselves

Skip this if your upload is slow; it moves about 1 GB.

1. Open Reddit's Help Center article on video uploads and record the size and length limits it
   states today, with the date.
2. With a source longer than 15 minutes:

   ```bash
   rmedia clip long.mp4 -s 0 -e 16:00 --fit reddit-video -o fit-check
   # error: … the range (16:00) is longer than reddit-video allows (15:00); use --fit-allow-trim …
   rmedia clip long.mp4 -s 0 -e 16:00 --fit reddit-video --fit-allow-trim -o fit-check
   ```

   Upload the trimmed 15:00 file. It should be accepted.

## Results Sheet

Copy this, fill it in, and send it back. The maintainer copies it into
`docs/operations/fit-owner-checks-results.md` and `specs/005-budgeted-reddit-video/checklists/validation.md`
(SC-008), and ticks the roadmap.

```text
Date:                     
rmedia version / OS:      
Source (length, size):    

A  result line:           
   accepted / processed:  
   web plays, sound:      
   app plays, sound:      
B  result line:           
   bytes (<= 20000000):   
   accepted / processed:  
   web plays, sound:      
   app plays, sound:      
C  result line:           
   accepted / processed:  
   app full screen, sound:
Optional: Help Center limits (date):        
Optional: 15:00 trimmed file accepted:      
Notes:                    
```
