<?php
// PHP Admin SDK — Contract Test Runner

declare(strict_types=1);

require_once __DIR__ . '/../../herald-admin-php/autoload.php';

use Herald\Admin\HeraldAdmin;

function loadSpec(): array {
    $specPath = getenv('HERALD_SPEC_PATH') ?: __DIR__ . '/../spec/spec.json';
    return json_decode(file_get_contents($specPath), true, 512, JSON_THROW_ON_ERROR);
}

function resolveValue(mixed $val, array $saved): mixed {
    if (is_string($val) && str_starts_with($val, '$')) {
        $ref = substr($val, 1);
        if ($ref === 'event_seq_minus_1') {
            return $saved['event_seq'] - 1;
        }
        return $saved[$ref] ?? null;
    }
    return $val;
}

function resolveInput(array $inp, array $saved): array {
    $result = [];
    foreach ($inp as $k => $v) {
        $result[$k] = resolveValue($v, $saved);
    }
    return $result;
}

function typeOf(mixed $val): string {
    if ($val === null) return 'null';
    if (is_bool($val)) return 'boolean';
    if (is_int($val) || is_float($val)) return 'number';
    if (is_string($val)) return 'string';
    if (is_array($val)) {
        if (array_is_list($val)) return 'array';
        return 'object';
    }
    return gettype($val);
}

function validateFields(array $obj, array $fields): array {
    $errors = [];
    foreach ($fields as $key => $expectedType) {
        if (!array_key_exists($key, $obj)) {
            $errors[] = "missing field: $key";
            continue;
        }
        if ($expectedType === 'any') continue;
        $actual = typeOf($obj[$key]);
        if ($expectedType === 'object' && ($actual === 'object' || $actual === 'null')) continue;
        if ($actual !== $expectedType) {
            $errors[] = "$key: expected type $expectedType, got $actual";
        }
    }
    return $errors;
}

function validateExpect(mixed $result, array $expect, array $saved): array {
    $errors = [];

    if ($expect['void'] ?? false) return $errors;

    if (($expect['type'] ?? '') === 'array') {
        if (!is_array($result) || !array_is_list($result)) {
            return ['expected array, got ' . typeOf($result)];
        }
        if (isset($expect['length']) && count($result) !== $expect['length']) {
            $errors[] = "expected array length {$expect['length']}, got " . count($result);
        }
        if (isset($expect['min_length']) && count($result) < $expect['min_length']) {
            $errors[] = "expected array min length {$expect['min_length']}, got " . count($result);
        }
        if (isset($expect['contains']) && !in_array($expect['contains'], $result, true)) {
            $errors[] = "expected array to contain '{$expect['contains']}'";
        }
        if (isset($expect['item_fields']) && count($result) > 0) {
            $errors = array_merge($errors, validateFields($result[0], $expect['item_fields']));
        }
        return $errors;
    }

    $obj = $result;
    if (!is_array($obj) && isset($expect['fields'])) {
        return ['expected object, got ' . typeOf($result)];
    }

    if (isset($expect['fields'])) {
        $errors = array_merge($errors, validateFields($obj, $expect['fields']));
    }

    if (isset($expect['values'])) {
        foreach ($expect['values'] as $key => $expected) {
            $resolved = resolveValue($expected, $saved);
            $actual = $obj[$key] ?? null;
            if ($actual !== $resolved) {
                $errors[] = "$key: expected " . json_encode($resolved) . ", got " . json_encode($actual);
            }
        }
    }

    if (isset($expect['events_length'])) {
        $events = $obj['events'] ?? [];
        if (!is_array($events)) {
            $errors[] = 'expected events array';
        } elseif (count($events) !== $expect['events_length']) {
            $errors[] = "expected events length {$expect['events_length']}, got " . count($events);
        }
    }

    if (isset($expect['events_min_length'])) {
        $events = $obj['events'] ?? [];
        if (!is_array($events)) {
            $errors[] = 'expected events array';
        } elseif (count($events) < $expect['events_min_length']) {
            $errors[] = "expected events min length {$expect['events_min_length']}, got " . count($events);
        }
    }

    if (isset($expect['event_fields'])) {
        $events = $obj['events'] ?? [];
        if (is_array($events) && count($events) > 0) {
            $errors = array_merge($errors, validateFields($events[0], $expect['event_fields']));
        }
    }

    if (isset($expect['first_event_values'])) {
        $events = $obj['events'] ?? [];
        if (is_array($events) && count($events) > 0) {
            foreach ($expect['first_event_values'] as $key => $expected) {
                $resolved = resolveValue($expected, $saved);
                if (($events[0][$key] ?? null) !== $resolved) {
                    $errors[] = "first event $key: expected " . json_encode($resolved) . ", got " . json_encode($events[0][$key] ?? null);
                }
            }
        }
    }

    if (isset($expect['first_event_fields'])) {
        $events = $obj['events'] ?? [];
        if (is_array($events) && count($events) > 0) {
            $errors = array_merge($errors, validateFields($events[0], $expect['first_event_fields']));
        }
    }

    return $errors;
}

function executeOperation(HeraldAdmin $client, string $op, array $inp): mixed {
    return match ($op) {
        'health' => $client->health(),
        'streams.create' => $client->streams->create($inp['id'], $inp['name'], $inp['meta'] ?? null, $inp['public'] ?? false),
        'streams.get' => $client->streams->get($inp['id']),
        'streams.list' => $client->streams->list(),
        'streams.update' => (function() use ($client, $inp) {
            $client->streams->update($inp['id'], $inp['name'] ?? null, $inp['meta'] ?? null, $inp['archived'] ?? null);
            return null;
        })(),
        'streams.delete' => (function() use ($client, $inp) { $client->streams->delete($inp['id']); return null; })(),
        'members.add' => $client->members->add($inp['stream_id'], $inp['user_id'], $inp['role'] ?? 'member'),
        'members.list' => $client->members->list($inp['stream_id']),
        'members.update' => (function() use ($client, $inp) { $client->members->update($inp['stream_id'], $inp['user_id'], $inp['role']); return null; })(),
        'members.remove' => (function() use ($client, $inp) { $client->members->remove($inp['stream_id'], $inp['user_id']); return null; })(),
        'events.publish' => $client->events->publish($inp['stream_id'], $inp['sender'], $inp['body'], $inp['meta'] ?? null, $inp['parent_id'] ?? null),
        'events.list' => $client->events->list($inp['stream_id'], isset($inp['before']) ? (int)$inp['before'] : null, isset($inp['after']) ? (int)$inp['after'] : null, isset($inp['limit']) ? (int)$inp['limit'] : null, $inp['thread'] ?? null),
        'events.edit' => (function() use ($client, $inp) { $client->events->edit($inp['stream_id'], $inp['event_id'], $inp['body']); return null; })(),
        'events.delete' => (function() use ($client, $inp) { $client->events->delete($inp['stream_id'], $inp['event_id']); return null; })(),
        'events.getReactions' => $client->events->getReactions($inp['stream_id'], $inp['event_id']),
        'events.trigger' => (function() use ($client, $inp) { $client->events->trigger($inp['stream_id'], $inp['event'], $inp['data'] ?? null); return null; })(),
        'presence.getUser' => $client->presence->getUser($inp['user_id']),
        'presence.getStream' => $client->presence->getStream($inp['stream_id']),
        'presence.getCursors' => $client->presence->getCursors($inp['stream_id']),
        'presence.getBulk' => $client->presence->getBulk($inp['user_ids']),
        'presence.setOverride' => $client->presence->setOverride($inp['user_id'], ['status' => $inp['status']]),
        'blocks.block' => (function() use ($client, $inp) { $client->chat->blocks->block($inp['user_id'], $inp['blocked_id']); return null; })(),
        'blocks.unblock' => (function() use ($client, $inp) { $client->chat->blocks->unblock($inp['user_id'], $inp['blocked_id']); return null; })(),
        'blocks.list' => $client->chat->blocks->list($inp['user_id']),
        default => throw new \RuntimeException("Unknown operation: $op"),
    };
}

// --- Main ---
$spec = loadSpec();
$url = getenv('HERALD_URL') ?: 'http://127.0.0.1:16300';
$apiToken = getenv('HERALD_API_TOKEN');
if (!$apiToken) {
    fwrite(STDERR, "HERALD_API_TOKEN is required — run via run-all.ts or set it manually\n");
    exit(1);
}

$client = new HeraldAdmin($url, token: $apiToken);

$saved = [];
$passed = 0;
$failed = 0;
$total = 0;
foreach ($spec['groups'] as $g) {
    $total += count($g['cases']);
}

echo "\n  PHP Admin SDK Contract Tests ($total cases)\n";
echo "  " . str_repeat('-', 50) . "\n";

foreach ($spec['groups'] as $group) {
    foreach ($group['cases'] as $tc) {
        $tcId = $tc['id'];
        $inp = resolveInput($tc['input'] ?? [], $saved);

        try {
            $result = executeOperation($client, $tc['operation'], $inp);
        } catch (\Throwable $e) {
            $failed++;
            echo "  FAIL [php] $tcId: {$e->getMessage()}\n";
            continue;
        }

        if (isset($tc['save']) && is_array($result)) {
            foreach ($tc['save'] as $alias => $field) {
                $saved[$alias] = $result[$field];
            }
        }

        $errs = validateExpect($result, $tc['expect'], $saved);
        if ($errs) {
            $failed++;
            echo "  FAIL [php] $tcId: " . implode('; ', $errs) . "\n";
        } else {
            $passed++;
            echo "  ok [php] $tcId\n";
        }
    }
}

echo "\n  PHP: $passed passed, $failed failed\n\n";
exit($failed > 0 ? 1 : 0);
