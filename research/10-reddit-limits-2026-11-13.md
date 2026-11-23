# 10. Reddit post limits (as of 2026-11-13)

Recorded 2026-11-19 for feature 005 (budgeted Reddit video) and roadmap item 3.

**Source:** [allplatforms.io/reddit](https://allplatforms.io/reddit/), "Last verified: November 13,
2026". It cites Reddit Help (`support.reddithelp.com/hc/en-us/articles/360060422572`) and Reddit
Ads' ad unit specifications (`business.reddithelp.com/articles/Knowledge/Reddit-Ad-Unit-Specifications`).
The page was read in a browser on 2026-11-19 (it is behind a Cloudflare check, so curl and
WebFetch get a 403). It's a third-party summary that says it checked Reddit's documentation. It
replaces the [U-3P] guide figures in `research/05-gif-export.md` §1 where the two overlap. Reddit
Help itself is still not read directly (it 403s here too).

Status key: **[V-1113]** = stated on the page above as of 2026-11-13. **[O-1123]** = found by the
owner's real Reddit uploads, reported 2026-11-23. **[U]** = still not stated by any source we have.

**Update 2026-11-23:** the owner's upload tests found that Reddit's hard limit for GIFs is 100 MB,
not the 20 MB image cap the page gives for "images (incl. GIF)". This matches the 100 MB
(i.redd.it animated GIF) figure in `research/05-gif-export.md` §1. The `reddit-post` target's hard
cap is now 100 MB and the target is `verified`.

## Organic posts (what rmedia targets)

| Limit | Value | Status |
|---|---|---|
| Video: max file size | 1 GB | [V-1113] |
| Video: max duration | 15 min (30 min with Reddit Premium) | [V-1113] |
| Video: containers | MP4, MOV | [V-1113] |
| Video: codec | H.264 | [V-1113] |
| Video: audio | optional, supported | [V-1113] |
| Video: resolution | 1920 × 1080 recommended (not a maximum) | [V-1113] |
| Video: aspect ratio | any; 16:9 displays best | [V-1113] |
| Video: looping | "GIF-style looping also available" | [V-1113] |
| Video: frame rate, bitrate, min duration | not stated for organic posts | [U] |
| Image: max file size | 20 MB | [V-1113] |
| GIF (post): max file size | **100 MB** (not 20 MB) | [O-1123] |
| Image: max dimensions | 4000 × 4000 px | [V-1113] |
| Image: formats | JPG, PNG, GIF | [V-1113] |
| Image: aspect ratio | any; 1.91:1 recommended for the feed | [V-1113] |
| Gallery | up to 20 images, 20 MB each, caption up to 180 characters each | [V-1113] |
| Comment GIF / image limits | not stated | [U] |
| Animated WebP | not listed (only JPG, PNG, GIF) | [U], assume not supported |
| Title | 300 characters, required | [V-1113] |
| Text body | 40,000 characters (80,000 with Premium) | [V-1113] |
| Comment text | 10,000 characters | [V-1113] |

## Promoted posts (ads): not rmedia targets, recorded for reference

| Limit | Value |
|---|---|
| Video | 1 GB; MP4, MOV; 2 s to 15 min; **max 30 fps**; ProRes not supported |
| Video resolution | 1440 × 1080 (4:3) or 1920 × 1080 (16:9) cross-device; mobile 1:1 1200 × 1200, 3:4 900 × 1200, 4:5 1200 × 1500 |
| Image | 3 MB; JPG, PNG, GIF; 1440 × 1080 (4:3) recommended |
| Carousel | 2–6 cards; 20 MB per static card, 3 MB per GIF card; caption 50 characters |
| Headline / body | 300 / 500 characters |

## What this means for rmedia

- **GIF targets (003).** *Superseded 2026-11-23:* `reddit-post` is now 8 MB soft, 100 MB hard and
  `verified` (owner upload, [O-1123]). Before that it was 8 MB soft, 20 MB hard, matching the image
  cap above. `reddit-comment` (3 MB soft, 8 MB hard) is still [U]: no source states a
  comment limit. It keeps `verified: false` until Reddit Help itself is read or a real comment
  upload passes (roadmap item 3). A third-party summary isn't enough to change the
  flag (003 FR-007).
- **Budgeted Reddit video (005).** The hard caps are 1 GB and 15 min, MP4 (or MOV), H.264, with
  audio allowed. 1080p is the recommended size. Organic posts state no frame-rate or bitrate limit.
  The ads' 30 fps cap doesn't apply to organic posts. Treat it as a soft default only if real
  uploads show a problem. Premium's 30 min isn't the default, because rmedia can't know a user's
  plan. It is a data field the user can change.
- **Practical budgets.** The 8–10 MB and 60 s subreddit AutoMod figures in `research/05` are
  conventions of individual subreddits, not Reddit limits. Targets stay data, so a user can set
  them per subreddit.

## Still to check (owner, roadmap item 3)

- Open Reddit Help "What are the image and video file size and format requirements" in a
  browser and compare it with the table above.
- Real uploads: a 005 video near the 1 GB / 15 min caps (or a smaller one with audio) and a
  comment GIF. (Post GIF: done 2026-11-23, 100 MB limit.)
