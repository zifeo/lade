# Lade Architecture

This document provides an overview of Lade's internal architecture, explaining how commands are intercepted, how configurations are resolved, and how secrets are securely injected and masked.

## 1. High-Level Flow (Shell Hooks)

When a user runs `lade on`, Lade registers a pre-execution hook in their shell (Bash, Zsh, or Fish). This hook intercepts commands before they are run to check if they require secrets.

```mermaid
sequenceDiagram
    participant User
    participant Shell as Shell (Bash/Zsh/Fish)
    participant Lade as Lade CLI
    participant Config as lade.yml
    participant Providers as Vaults (1Password, etc.)

    User->>Shell: Types `my-command`
    Shell->>Lade: Pre-exec hook: `lade set my-command`
    Lade->>Config: Parse & merge configurations
    Config-->>Lade: Matching rules & URIs
    
    alt Command matches a rule
        Lade->>Providers: Fetch secrets (concurrently)
        Providers-->>Lade: Raw secret values
        Lade-->>Shell: Returns `export VAR=secret`
        Shell->>Shell: Evaluates exports
    else No match
        Lade-->>Shell: Returns empty string
    end
    
    Shell->>Shell: Executes `my-command`
    Shell->>Lade: Post-exec hook: `lade unset my-command`
    Lade-->>Shell: Returns `unset VAR`
    Shell->>Shell: Cleans up environment
```

## 2. Configuration Resolution

Lade traverses the directory tree upwards to find and merge all `lade.yml` files. It then evaluates the rules against the current command.

```mermaid
flowchart TD
    Start[Command: `npm run build`] --> Find[Find all `lade.yml` from CWD to Git Root]
    Find --> Merge[Merge configs (deep merge)]
    Merge --> Match{Regex matches command?}
    
    Match -- Yes --> UserCheck{Is user specified?}
    UserCheck -- Yes --> ResolveUser[Resolve for specific user or fallback to `.']
    UserCheck -- No --> ResolveUser
    
    ResolveUser --> Loaders[Dispatch to Loaders]
    
    Match -- No --> Skip[Skip rule]
    
    Loaders --> |op://| OpLoader[1Password CLI]
    Loaders --> |vault://| VaultLoader[HashiCorp Vault]
    Loaders --> |file://| FileLoader[Local File]
    Loaders --> |Raw| RawLoader[Plaintext]
```

## 3. Execution & Masking (`lade inject`)

When using `lade inject` (or in environments where shell hooks aren't available), Lade wraps the command execution. It uses a pseudo-terminal (PTY) to capture the output and redact secrets on the fly.

```mermaid
sequenceDiagram
    participant User
    participant Lade as Lade (Parent)
    participant PTY as Pseudo-Terminal
    participant Child as Subprocess
    
    User->>Lade: `lade inject my-command`
    Lade->>Lade: Resolve secrets
    Lade->>PTY: Create PTY pair
    Lade->>Child: Spawn `my-command` with injected ENV
    
    Child->>PTY: Write output (contains secret)
    PTY->>Lade: Read stream
    
    Lade->>Lade: Aho-Corasick Redactor finds secret
    Lade->>Lade: Replace secret with `REDACTED`
    
    Lade->>User: Print sanitized output
```
