# Chat wallpapers

Chat wallpaper selection remains an appearance setting shared by the local
client's conversations. Preset gradients now cover all four corners.

Android renders the wallpaper in a non-interactive layer behind the conversation,
covering the window's content insets as well as the message list. Its bounds track
layout changes. Other screens retain their own backgrounds.

Custom images use centered cover scaling, preserving proportions without empty
edges. Android uses its system photo picker and stores a normalized JPEG in the
app's private files through AtomicFile. Windows stores a normalized JPEG Blob in
IndexedDB and uses an object URL rather than adding image data to preferences.
Images are downsampled to a maximum side of 2048 pixels and re-encoded, removing
source EXIF metadata. Wallpaper photos are not sent to the messaging server.

Users can select the saved photo again, replace or remove it, choose another
preset, and adjust intensity. English labels are included.

Pixel tests cover persistence across Activity recreation, removal, full-window
geometry and corner pixels, plus coverage of both gradient presets. A headless
Edge check covers Windows image normalization, resizing, persistence across page
reload, removal, and the actual chat background CSS.
