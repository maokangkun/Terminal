# Vendor Directory

`vendor/servo/` is reserved for a full Servo source checkout.

The current repository ignores that directory because Servo is large and should
be fetched intentionally:

```sh
./scripts/fetch-servo.sh
```

Native in-process Servo integration requires the full Servo workspace, not only
selected HTML/layout files.
