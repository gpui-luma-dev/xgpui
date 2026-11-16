# WebKit Flowers reference images

Source and attribution: https://webkit.org/blog-files/color-gamut/

The three original WebKit JPEGs are retained unchanged. These image assets are credited to WebKit and are separate from the application code license.

The color lab (`just color --flowers`) decodes them at runtime with the `image` crate, which applies no color management, and interprets the channels explicitly:

| File | Size | Embedded profile | Lab interpretation |
| --- | --- | --- | --- |
| `Flowers-ProPhoto.jpg` | 1200 × 795 | Kodak “ProPhoto RGB” v2: ROMM colorants, pure gamma 461/256, no linear toe | `ImageColor::PROPHOTO_RGB` (ROMM, with toe); deviation measured and shown |
| `Flowers-sRGB.jpg` | 1200 × 795 | sRGB IEC61966-2.1 | `ImageColor::SRGB` |
| `Flowers-oog.jpg` | 1644 × 1088 | Apple display profile (screen capture) | Reference visualization only, shown as encoded sRGB |

| File | SHA-256 |
| --- | --- |
| `Flowers-ProPhoto.jpg` | `37fcbf81abe112b92ad8b26be3ec0b7346048f31888183547ad48ba07f36f88e` |
| `Flowers-oog.jpg` | `eb8a6c7601d0bb85c04d41441563acd1596f1e7f2568e6433fc70187d5ed3017` |
| `Flowers-sRGB.jpg` | `6f5cdc2807550508eae156ff6e449d37b21010aa53553572e38600fc303864b6` |

The highlighted image is WebKit’s supplied visualization, not a locally generated mask; the lab also draws its own pre-mapping overlay with its own criteria.
