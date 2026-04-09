# Herald Contract Tests

Wire-protocol compliance tests for Herald admin SDKs. Runs the same test spec against all four SDK implementations to prevent drift.

## How It Works

1. **`spec/spec.json`** defines the test cases: groups of operations with expected inputs and outputs
2. **`harness.ts`** starts a real Herald server on `localhost:16301`
3. **`run-all.ts`** orchestrates: starts server, runs all SDK test suites, aggregates results
4. Individual runners execute spec cases against each SDK:
   - `run-typescript.ts` — TypeScript admin SDK
   - `go/contract_test.go` — Go admin SDK
   - `python/test_contract.py` — Python admin SDK
   - `ruby/test_contract.rb` — Ruby admin SDK

## Running

```bash
# Prerequisites: Herald release binary built
cargo build --release

# Run all SDKs
cd contract-tests
npm install
npx tsx run-all.ts

# Run a single SDK
npx tsx run-typescript.ts
cd go && go test ./...
cd python && python3 test_contract.py
cd ruby && ruby test_contract.rb
```

Environment: `SHROUDB_MASTER_KEY` must be set (any 64-char hex string for test).

## Spec Format

`spec/spec.json` contains test groups:

```json
{
  "groups": [
    {
      "name": "streams",
      "auth": "admin",
      "tests": [
        {
          "id": "create-stream",
          "op": "streams.create",
          "input": { "id": "test-stream", "name": "Test" },
          "expect": { "id": "test-stream", "name": "Test" },
          "save": { "streamId": "$.id" }
        }
      ]
    }
  ]
}
```

- **`op`**: SDK method to call (e.g. `streams.create`, `events.publish`)
- **`input`**: Arguments passed to the SDK method
- **`expect`**: Validation rules for the response (field presence, values, types)
- **`save`**: Extract values from the response for use in subsequent tests (e.g. `$streamId`)

## Adding a New SDK

1. Create a test runner file (e.g. `csharp/ContractTest.cs`)
2. Load `spec/spec.json` and iterate test groups
3. For each test case: call the SDK method, validate against `expect`
4. Add the runner to `run-all.ts` as a subprocess
5. Validation helpers are in `validate.ts` — port the logic to your SDK's language
