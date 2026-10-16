"""Fixed synthetic/public-identifier error-cluster checks; model outputs only."""

import argparse
import json
from pathlib import Path

import numpy as np
from tokenizers import Tokenizer

from extract import MODEL_ROOT, SPECS, encode, session, softmax

CASES = {
    "model_json": '{"model": "claude-sonnet-4-5"}',
    "model_prose": "Use claude-sonnet-4-5 for this request.",
    "lowercase_person": "Please contact anna schmidt tomorrow.",
    "capitalised_person": "Please contact Anna Schmidt tomorrow.",
    "records": "Recipient: Anna Schmidt\nOwner: Mira Velloran",
    "code_symbol": "class AnnaSchmidt: pass",
    "code_contact": '{"full_name": "anna schmidt"}',
    "zwsp_identifier": "Tax ID: 123\u200b456\u200b7890",
    "ordinary_word": "May we run the code now?",
    "ambiguous_person": "Her given name is May.",
    "organisation": "The company Mira Velloran filed its accounts.",
}


def run(output):
    result = {"scope": "fixed synthetic fixtures/public product identifier; original pinned model token argmax and confidence only; no claim of Gaze runtime reproduction, decoding, policy or exact span semantics", "models": {}}
    for model, spec in SPECS.items():
        directory = MODEL_ROOT / spec["directory"]
        runtime = session(directory / spec["file"])
        tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
        tokenizer.no_truncation()
        tokenizer.no_padding()
        labels = json.loads((directory / "config.json").read_text())["id2label"]
        records = []
        for name, text in CASES.items():
            encoding, feed = encode(tokenizer, text, runtime)
            logits = runtime.run(["logits"], feed)[0][0]
            assert np.isfinite(logits).all()
            probabilities = softmax(logits)
            found = []
            for (a, b), row in zip(encoding.offsets, probabilities, strict=True):
                index = int(row.argmax())
                tag = labels[str(index)]
                if a < b and tag != "O":
                    found.append({"span": [a, b], "text": text[a:b], "label": tag, "confidence": float(row[index])})
            records.append({"case": name, "text": text, "tokens": found})
        result["models"][model] = records
        del runtime
    output.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({"cases": len(CASES) * len(SPECS), "failures": 0}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args().output)
