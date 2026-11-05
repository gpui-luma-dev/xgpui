# xgpui-font-inspect

Internal implementation helpers for xgpui's Apple, DirectWrite, and Parley text
backends. Reads GSUB/GPOS feature metadata and fvar axis metadata into GPUI's
public inspection data types. Parsing errors remain distinct from absent tables.

This crate is not re-exported by `gpui`; applications should use
`TextSystem::opentype_features` and `TextSystem::font_axes`. Its doc-hidden
functions are backend implementation details, not application API. It is
packaged separately so published backends can share the same implementation.

Parser tests include malformed/truncated metadata, empty results, source
merging, signed axis coordinates, and the repository's licensed IBM Plex fixture.
