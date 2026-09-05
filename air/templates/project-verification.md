# __APPNAME__ Verification

## Baseline

```bash
air build
air check
__RUN_COMMAND__
air dist
```

## Automated Checks

- [ ] Replace with commands and expected output/exit status.

## Manual Checks

- [ ] Replace with visible behavior to exercise for this target.

## Failure And Safety Checks

- [ ] Invalid input and provider errors are visible and do not corrupt state.
- [ ] No secrets, private data, generated output, or local vendor cache appear
      in `git status` before commit.
