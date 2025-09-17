$ErrorActionPreference = "Stop"
Set-Location (git rev-parse --show-toplevel)
# eenv PreCommit
eenv PreCommit --write