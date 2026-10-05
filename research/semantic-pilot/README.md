# Synthetic semantic pilot

The portable dataset contains 48 bilingual semantic cases in 24 families and 24 routing controls. All directory names are invented. Labels are author judgements; the split is operational and is not blind human gold. A fixed shortlist tests conditional decision behavior, not product retrieval or genuine-user effectiveness.

## Offline checks

```sh
python3 -m unittest discover -s research/semantic-pilot -p 'test_*.py'
python3 research/semantic-pilot/pilot.py prepare --out local-data/pilot/preview.jsonl
```

Outputs use exclusive creation and belong under ignored local-data. Prepared requests contain only whitelisted query, cwd and candidate fields. The dataset manifest binds current synthetic input hashes.

## Separate live runs

A live run requires an explicitly authorized credential, account and bounded request plan. Keep the credential in TYPESAFE_API_KEY, never in arguments, tracked files or logs. Freeze the split, order, model, request identities and budgets before calling the runner. Offline preparation grants no live authority.

```sh
python3 research/semantic-pilot/pilot.py freeze --split development --max-requests 16 --max-seconds 300 --out local-data/pilot/run-manifest.json
python3 research/semantic-pilot/pilot.py run --split development --run-manifest local-data/pilot/run-manifest.json --authorized-live --max-requests 16 --max-seconds 300 --out local-data/pilot/results.jsonl
python3 research/semantic-pilot/pilot.py score --split development --run-manifest local-data/pilot/run-manifest.json --results local-data/pilot/results.jsonl --out local-data/pilot/summary.json
```

The runner refuses redirects, bounds payload/response sizes, records validated fields and stops on transport/HTTP/schema failure without retries. Research timeouts differ from the product interactive deadline. A partial file must not be blindly replayed.

## Interpretation

Report attempted and planned denominators, wrong destinations, required abstention, malformed responses, missing cases and latency. Reversed-order repeats measure order sensitivity and are not independent samples. Hashes bind local inputs and results but do not authenticate provider origin. Mock tests and synthetic decisions never approve automatic navigation.
