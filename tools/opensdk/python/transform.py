#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0

import base64
import json
import sys

encoded = sys.stdin.buffer.read(1024 * 1024 + 1)
if len(encoded) > 1024 * 1024:
    raise ValueError("input exceeds 1 MiB")
value = json.loads(encoded)
if (
    not isinstance(value, dict)
    or set(value) != {"source", "prefix"}
    or not isinstance(value["source"], str)
    or not isinstance(value["prefix"], str)
):
    raise ValueError("input must contain exactly source and prefix strings")
if len(value["source"]) > 64 * 1024 or len(value["prefix"]) > 1024:
    raise ValueError("transform input exceeds field limits")
if not value["source"].isascii() or not value["prefix"].isascii():
    raise ValueError("uppercase-v1 accepts ASCII input only")

transformed = f'{value["prefix"]}{value["source"].upper()}'.encode("utf-8")
output = {
    "schema": "dev.pliegors.build-transform/v1",
    "mediaType": "text/plain; charset=utf-8",
    "bytesBase64": base64.b64encode(transformed).decode("ascii"),
}
print(json.dumps(output, ensure_ascii=True, separators=(",", ":")))
