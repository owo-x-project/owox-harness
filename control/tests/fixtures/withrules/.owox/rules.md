# Rules

## Change policy

- Match the existing style, naming, and granularity
- Record a decision before large structural changes

## Dependency policy

- Check whether the standard library can do it first

## Deletion policy

- Generated artifacts may be deleted; canon and history need human confirmation

## Safety

- Never put secrets in artifacts, logs, or outbound requests

## Irreversible operations

- git push --force / rewriting history: destroys others' history and is hard to recover
- terraform destroy: tears down all provisioned infrastructure
  detect: \bterraform\s+destroy\b

## Human gates

- Editing the canon (.owox/): value, policy, and rule changes need human judgment
