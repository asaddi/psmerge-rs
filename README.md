# psmerge #

Simple AWS Parameter Store & Secrets Manager templating. (Also another opportunity to exercise Rust!)

Needless to say, this is a toy! And I'm sure there are a billion other, better supported programs that do the same thing (and probably more). Do not use in production! ;)

## Configuration File Example ##

Create a YAML file somewhere along with your Jinja2 templates. (Note: Jinja2 support is as good as the [tera](https://crates.io/crates/tera) crate's. YMMV.)

    region: us-west-2
    parameter_store_prefixes:
      # Scanned in order, later ones take precedence
      - /Global
      - /TestApp
    secrets:
      # Scanned in order, later ones take precedence
      # Each secret is expected to be a JSON object (i.e. as created from the console)
      - MySecret1
      - MySecret2
    templates:
      - src: relative/path/from/config/template1.j2
        out: /path/to/destination1
      - src: /some/absolute/path/template2.j2
        out: /path/to/destination2

Everything except `templates` are optional.

## Synopsis ##

    psmerge /path/to/config.yaml

Files are only overwritten if there are actually changes.

Existing files are backed up with the `~` suffix (i.e. Emacs-style).

## To Do ##

 * Unix owner/group/mode (per template)
 * Default template output name (strip `.j2` extension, render in same directory)
 * Additional suffix support, which are appended to Parameter Store prefixes & Secrets Manager secret names. For example, suffixes `aaa` & `bbb` result in scanning: `/Global`, `/Global_aaa`, `/Global_bbb`, etc.
