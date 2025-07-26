# Library management

Audiocontrol allows not just to control players, but also browse music libraries if the platforms supports this. The supported platforms today are mpd and LMS.

## Entity Relationships

The Audiocontrol library system manages three primary entities and their relationships:

```
+-------------+       +-------------+       +-------------+
|   Artist    |       |    Album    |       |    Track    |
+-------------+       +-------------+       +-------------+
| id          |<----->| id          |<----->| name        |
| name        |   |   | name        |   |   | track_number|
| is_multi    |   |   | artists     |   |   | disc_number |
| metadata    |   |   | release_date|   |   | artist      |
+-------------+   |   | tracks      |   |   | uri         |
                  |   | cover_art   |   |   +-------------+
                  |   | uri         |   |
                  |   +-------------+   |
                  |                     |
                  +---------------------+
                      Many-to-Many
```

### Relationships

1. **Artist to Album** (Many-to-Many):
   - One artist can create multiple albums
   - One album can be created by multiple artists (compilations, collaborations)
   - Albums store a reference to their artists
   - Artists may have metadata about their albums

2. **Album to Track** (One-to-Many):
   - One album contains multiple tracks
   - Each track belongs to one album
   - Albums maintain a list of their tracks
   - Tracks can reference a different artist than the album artist

3. **Artist to Track** (Optional One-to-One):
   - Tracks can optionally have their own artist (different from album artist)
   - This relationship is only stored when the track artist differs from the album artist

### Implementation Notes

- The Album structure stores both artists and tracks in thread-safe containers (`Arc<Mutex<Vec<...>>>`)
- The `artists_flat` field in Album provides a convenience representation of multiple artists as a single string
- Track numbers and disc numbers are optional to support streaming content and various music formats

