// C# Admin SDK — Contract Test Runner
using System.Text.Json;
using Herald.Admin;

var specPath = Environment.GetEnvironmentVariable("HERALD_SPEC_PATH")
    ?? Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "spec", "spec.json");
var spec = JsonDocument.Parse(File.ReadAllText(specPath)).RootElement;

var url = Environment.GetEnvironmentVariable("HERALD_URL") ?? "http://127.0.0.1:16300";
var apiToken = Environment.GetEnvironmentVariable("HERALD_API_TOKEN");
if (string.IsNullOrEmpty(apiToken))
{
    Console.Error.WriteLine("HERALD_API_TOKEN is required");
    return 1;
}

var client = new HeraldAdmin(url, token: apiToken);
var saved = new Dictionary<string, object>();
int passed = 0, failed = 0;

int total = 0;
foreach (var g in spec.GetProperty("groups").EnumerateArray())
    total += g.GetProperty("cases").GetArrayLength();

Console.WriteLine($"\n  C# Admin SDK Contract Tests ({total} cases)");
Console.WriteLine("  " + new string('-', 50));

foreach (var group in spec.GetProperty("groups").EnumerateArray())
{
    foreach (var tc in group.GetProperty("cases").EnumerateArray())
    {
        var tcId = tc.GetProperty("id").GetString()!;
        var operation = tc.GetProperty("operation").GetString()!;
        var input = tc.TryGetProperty("input", out var inp) ? ResolveInput(inp, saved) : new Dictionary<string, object?>();

        object? result;
        try
        {
            result = await ExecuteOperation(client, operation, input);
        }
        catch (Exception e)
        {
            failed++;
            Console.WriteLine($"  FAIL [cs] {tcId}: {e.Message}");
            continue;
        }

        // Save values
        if (tc.TryGetProperty("save", out var saveSpec) && result is Dictionary<string, object?> resultDict)
        {
            foreach (var prop in saveSpec.EnumerateObject())
            {
                if (resultDict.TryGetValue(prop.Value.GetString()!, out var val) && val != null)
                    saved[prop.Name] = val;
            }
        }

        var expect = tc.GetProperty("expect");
        var errors = ValidateExpect(result, expect, saved);
        if (errors.Count > 0)
        {
            failed++;
            Console.WriteLine($"  FAIL [cs] {tcId}: {string.Join("; ", errors)}");
        }
        else
        {
            passed++;
            Console.WriteLine($"  ok [cs] {tcId}");
        }
    }
}

Console.WriteLine($"\n  C#: {passed} passed, {failed} failed\n");
return failed > 0 ? 1 : 0;

// --- Helpers ---

static Dictionary<string, object?> ResolveInput(JsonElement inp, Dictionary<string, object> saved)
{
    var result = new Dictionary<string, object?>();
    foreach (var prop in inp.EnumerateObject())
    {
        result[prop.Name] = ResolveValue(prop.Value, saved);
    }
    return result;
}

static object? ResolveValue(JsonElement val, Dictionary<string, object> saved)
{
    if (val.ValueKind == JsonValueKind.String)
    {
        var s = val.GetString()!;
        if (s.StartsWith('$'))
        {
            var refName = s[1..];
            if (refName == "event_seq_minus_1")
                return Convert.ToInt64(saved["event_seq"]) - 1;
            return saved.GetValueOrDefault(refName);
        }
        return s;
    }
    return JsonHelper.Unwrap(val);
}

static string TypeOf(object? val) => val switch
{
    null => "null",
    bool => "boolean",
    int or long or double or float => "number",
    string => "string",
    System.Collections.IList => "array",
    Dictionary<string, object?> => "object",
    _ => val.GetType().Name,
};

static List<string> ValidateFields(Dictionary<string, object?> obj, JsonElement fields)
{
    var errors = new List<string>();
    foreach (var prop in fields.EnumerateObject())
    {
        var expectedType = prop.Value.GetString()!;
        if (!obj.ContainsKey(prop.Name))
        {
            errors.Add($"missing field: {prop.Name}");
            continue;
        }
        if (expectedType == "any") continue;
        var actual = TypeOf(obj[prop.Name]);
        if (expectedType == "object" && (actual == "object" || actual == "null")) continue;
        if (actual != expectedType)
            errors.Add($"{prop.Name}: expected type {expectedType}, got {actual}");
    }
    return errors;
}

static List<string> ValidateExpect(object? result, JsonElement expect, Dictionary<string, object> saved)
{
    var errors = new List<string>();

    if (expect.TryGetProperty("void", out var v) && v.GetBoolean())
        return errors;

    if (expect.TryGetProperty("type", out var typeEl) && typeEl.GetString() == "array")
    {
        if (result is not System.Collections.IList arrRaw)
            return new List<string> { $"expected array, got {TypeOf(result)}" };
        var arrList = arrRaw.Cast<object?>().ToList();
        if (expect.TryGetProperty("length", out var lenEl) && arrList.Count != lenEl.GetInt32())
            errors.Add($"expected array length {lenEl.GetInt32()}, got {arrList.Count}");
        if (expect.TryGetProperty("min_length", out var minEl) && arrList.Count < minEl.GetInt32())
            errors.Add($"expected array min length {minEl.GetInt32()}, got {arrList.Count}");
        if (expect.TryGetProperty("contains", out var containsEl))
        {
            var target = containsEl.GetString();
            if (!arrList.Any(x => x is string s && s == target))
                errors.Add($"expected array to contain '{target}'");
        }
        if (expect.TryGetProperty("item_fields", out var ifEl) && arrList.Count > 0 && arrList[0] is Dictionary<string, object?> firstItem)
            errors.AddRange(ValidateFields(firstItem, ifEl));
        return errors;
    }

    if (result is not Dictionary<string, object?> obj)
    {
        if (expect.TryGetProperty("fields", out _))
            return new List<string> { $"expected object, got {TypeOf(result)}" };
        return errors;
    }

    if (expect.TryGetProperty("fields", out var fieldsEl))
        errors.AddRange(ValidateFields(obj, fieldsEl));

    if (expect.TryGetProperty("values", out var valuesEl))
    {
        foreach (var prop in valuesEl.EnumerateObject())
        {
            var expected = ResolveValue(prop.Value, saved);
            obj.TryGetValue(prop.Name, out var actual);
            if (!Equals(actual, expected))
                errors.Add($"{prop.Name}: expected {JsonSerializer.Serialize(expected)}, got {JsonSerializer.Serialize(actual)}");
        }
    }

    // Extract events list once for all event-related validations
    List<object?>? evList = null;
    if (obj.TryGetValue("events", out var rawEvents) && rawEvents is System.Collections.IList rawList)
        evList = rawList.Cast<object?>().ToList();

    if (expect.TryGetProperty("events_length", out var evLenEl))
    {
        if (evList != null)
        {
            if (evList.Count != evLenEl.GetInt32())
                errors.Add($"expected events length {evLenEl.GetInt32()}, got {evList.Count}");
        }
        else errors.Add("expected events array");
    }

    if (expect.TryGetProperty("events_min_length", out var evMinEl))
    {
        if (evList != null)
        {
            if (evList.Count < evMinEl.GetInt32())
                errors.Add($"expected events min length {evMinEl.GetInt32()}, got {evList.Count}");
        }
        else errors.Add("expected events array");
    }

    if (expect.TryGetProperty("event_fields", out var efEl))
    {
        if (evList is { Count: > 0 } && evList[0] is Dictionary<string, object?> firstEf)
            errors.AddRange(ValidateFields(firstEf, efEl));
    }

    if (expect.TryGetProperty("first_event_values", out var fevEl))
    {
        if (evList is { Count: > 0 } && evList[0] is Dictionary<string, object?> firstFev)
        {
            foreach (var prop in fevEl.EnumerateObject())
            {
                var expected = ResolveValue(prop.Value, saved);
                firstFev.TryGetValue(prop.Name, out var actual);
                if (!Equals(actual, expected))
                    errors.Add($"first event {prop.Name}: expected {JsonSerializer.Serialize(expected)}, got {JsonSerializer.Serialize(actual)}");
            }
        }
    }

    if (expect.TryGetProperty("first_event_fields", out var fefEl))
    {
        if (evList is { Count: > 0 } && evList[0] is Dictionary<string, object?> firstFef)
            errors.AddRange(ValidateFields(firstFef, fefEl));
    }

    return errors;
}

static async Task<object?> ExecuteOperation(HeraldAdmin client, string op, Dictionary<string, object?> inp)
{
    string S(string key) => (string)inp[key]!;
    long? NL(string key) => inp.TryGetValue(key, out var v) && v != null ? Convert.ToInt64(v) : null;
    int? NI(string key) => inp.TryGetValue(key, out var v) && v != null ? Convert.ToInt32(v) : null;
    string? NS(string key) => inp.TryGetValue(key, out var v) ? v as string : null;

    switch (op)
    {
        case "health":
            var h = await client.HealthAsync();
            return JsonHelper.ToDict(h!.Value);

        case "streams.create":
            var sc = await client.Streams.CreateAsync(S("id"), S("name"), inp.GetValueOrDefault("meta"), inp.TryGetValue("public", out var pub) && pub is true);
            return JsonHelper.ToDict(sc!.Value);
        case "streams.get":
            var sg = await client.Streams.GetAsync(S("id"));
            return JsonHelper.ToDict(sg!.Value);
        case "streams.list":
            return await client.Streams.ListAsync();
        case "streams.update":
            await client.Streams.UpdateAsync(S("id"), NS("name"), inp.GetValueOrDefault("meta"), inp.TryGetValue("archived", out var arch) ? (bool?)arch : null);
            return null;
        case "streams.delete":
            await client.Streams.DeleteAsync(S("id"));
            return null;

        case "members.add":
            var ma = await client.Members.AddAsync(S("stream_id"), S("user_id"), NS("role") ?? "member");
            return JsonHelper.ToDict(ma!.Value);
        case "members.list":
            return await client.Members.ListAsync(S("stream_id"));
        case "members.update":
            await client.Members.UpdateAsync(S("stream_id"), S("user_id"), S("role"));
            return null;
        case "members.remove":
            await client.Members.RemoveAsync(S("stream_id"), S("user_id"));
            return null;

        case "events.publish":
            var ep = await client.Events.PublishAsync(S("stream_id"), S("sender"), S("body"), inp.GetValueOrDefault("meta"), NS("parent_id"));
            return JsonHelper.ToDict(ep!.Value);
        case "events.list":
            return await client.Events.ListAsync(S("stream_id"), NL("before"), NL("after"), NI("limit"), NS("thread"));
        case "events.edit":
            await client.Events.EditAsync(S("stream_id"), S("event_id"), S("body"));
            return null;
        case "events.delete":
            await client.Events.DeleteAsync(S("stream_id"), S("event_id"));
            return null;
        case "events.getReactions":
            return await client.Events.GetReactionsAsync(S("stream_id"), S("event_id"));
        case "events.trigger":
            await client.Events.TriggerAsync(S("stream_id"), S("event"), inp.GetValueOrDefault("data"));
            return null;

        case "presence.getUser":
            return await client.Chat.Presence.GetUserAsync(S("user_id"));
        case "presence.getStream":
            return await client.Chat.Presence.GetStreamAsync(S("stream_id"));
        case "presence.getCursors":
            return await client.Chat.Presence.GetCursorsAsync(S("stream_id"));

        case "blocks.block":
            await client.Chat.Blocks.BlockAsync(S("user_id"), S("blocked_id"));
            return null;
        case "blocks.unblock":
            await client.Chat.Blocks.UnblockAsync(S("user_id"), S("blocked_id"));
            return null;
        case "blocks.list":
            return await client.Chat.Blocks.ListAsync(S("user_id"));

        default:
            throw new InvalidOperationException($"Unknown operation: {op}");
    }
}
