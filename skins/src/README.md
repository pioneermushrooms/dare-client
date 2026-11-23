# Skin sources

`make_skin.py` draws the client's own start-screen skins and writes them as
Continuum-format `.skn` files (a BMP with the skin description appended). The art
is original and generated entirely by the script; it uses no third-party images.

Regenerate both bundled skins after editing the script:

```sh
python3 -I skins/src/make_skin.py "Dare" "assets/skins/Dare.skn"
python3 -I skins/src/make_skin.py "Dare Night" "assets/skins/Dare Night.skn"
```

`preview.py` renders a mock-up of a skin with ships, profile and zone text for
looking at a design before running the client:

```sh
python3 -I skins/src/preview.py "Dare" assets/classic/graphics/ships.bm2 preview.png 2
```

Players can add any other `.skn` skin by putting it in the `skins` folder beside the
client's settings file, or by listing a folder under `[skins] folders`.
