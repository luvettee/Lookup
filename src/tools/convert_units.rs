use std::collections::HashMap;
use serde_json::{json, Value};

struct UnitCategory {
    name: &'static str,
    units: &'static [(&'static str, f64, f64)],
}

const CATEGORIES: &[UnitCategory] = &[
    UnitCategory {
        name: "distance",
        units: &[
            ("mm", 0.001, 0.0),
            ("millimeter", 0.001, 0.0),
            ("millimeters", 0.001, 0.0),
            ("cm", 0.01, 0.0),
            ("centimeter", 0.01, 0.0),
            ("centimeters", 0.01, 0.0),
            ("m", 1.0, 0.0),
            ("meter", 1.0, 0.0),
            ("meters", 1.0, 0.0),
            ("km", 1000.0, 0.0),
            ("kilometer", 1000.0, 0.0),
            ("kilometers", 1000.0, 0.0),
            ("in", 0.0254, 0.0),
            ("inch", 0.0254, 0.0),
            ("inches", 0.0254, 0.0),
            ("ft", 0.3048, 0.0),
            ("foot", 0.3048, 0.0),
            ("feet", 0.3048, 0.0),
            ("yd", 0.9144, 0.0),
            ("yard", 0.9144, 0.0),
            ("yards", 0.9144, 0.0),
            ("mi", 1609.344, 0.0),
            ("mile", 1609.344, 0.0),
            ("miles", 1609.344, 0.0),
            ("nm", 1852.0, 0.0),
            ("nauticalmile", 1852.0, 0.0),
        ],
    },
    UnitCategory {
        name: "mass",
        units: &[
            ("mg", 0.001, 0.0),
            ("milligram", 0.001, 0.0),
            ("milligrams", 0.001, 0.0),
            ("g", 1.0, 0.0),
            ("gram", 1.0, 0.0),
            ("grams", 1.0, 0.0),
            ("kg", 1000.0, 0.0),
            ("kilogram", 1000.0, 0.0),
            ("kilograms", 1000.0, 0.0),
            ("oz", 28.349523125, 0.0),
            ("ounce", 28.349523125, 0.0),
            ("ounces", 28.349523125, 0.0),
            ("lb", 453.59237, 0.0),
            ("lbs", 453.59237, 0.0),
            ("pound", 453.59237, 0.0),
            ("pounds", 453.59237, 0.0),
            ("st", 6350.29318, 0.0),
            ("stone", 6350.29318, 0.0),
            ("t", 1000000.0, 0.0),
            ("tonne", 1000000.0, 0.0),
            ("tonnes", 1000000.0, 0.0),
        ],
    },
    UnitCategory {
        name: "volume",
        units: &[
            ("ml", 0.001, 0.0),
            ("milliliter", 0.001, 0.0),
            ("milliliters", 0.001, 0.0),
            ("l", 1.0, 0.0),
            ("liter", 1.0, 0.0),
            ("liters", 1.0, 0.0),
            ("tsp", 0.00492892159375, 0.0),
            ("tbsp", 0.01478676478125, 0.0),
            ("cup", 0.2365882365, 0.0),
            ("cups", 0.2365882365, 0.0),
            ("floz", 0.0295735295625, 0.0),
            ("fluidounce", 0.0295735295625, 0.0),
            ("gal", 3.785411784, 0.0),
            ("gallon", 3.785411784, 0.0),
            ("gallons", 3.785411784, 0.0),
            ("cm3", 0.001, 0.0),
            ("m3", 1000.0, 0.0),
        ],
    },
    UnitCategory {
        name: "speed",
        units: &[
            ("mps", 1.0, 0.0),
            ("meterspersecond", 1.0, 0.0),
            ("kmph", 0.2777777778, 0.0),
            ("kmh", 0.2777777778, 0.0),
            ("kph", 0.2777777778, 0.0),
            ("kilometersperhour", 0.2777777778, 0.0),
            ("mph", 0.44704, 0.0),
            ("milesperhour", 0.44704, 0.0),
            ("kn", 0.5144444444, 0.0),
            ("knot", 0.5144444444, 0.0),
            ("knots", 0.5144444444, 0.0),
        ],
    },
    UnitCategory {
        name: "temperature",
        units: &[
            ("c", 1.0, 0.0),
            ("celsius", 1.0, 0.0),
            ("f", 5.0 / 9.0, -32.0 * 5.0 / 9.0),
            ("fahrenheit", 5.0 / 9.0, -32.0 * 5.0 / 9.0),
            ("k", 1.0, -273.15),
            ("kelvin", 1.0, -273.15),
        ],
    },
    UnitCategory {
        name: "storage",
        units: &[
            ("b", 1.0, 0.0),
            ("byte", 1.0, 0.0),
            ("bytes", 1.0, 0.0),
            ("kb", 1000.0, 0.0),
            ("mb", 1000000.0, 0.0),
            ("gb", 1000000000.0, 0.0),
            ("tb", 1000000000000.0, 0.0),
            ("kib", 1024.0, 0.0),
            ("mib", 1048576.0, 0.0),
            ("gib", 1073741824.0, 0.0),
            ("tib", 1099511627776.0, 0.0),
        ],
    },
    UnitCategory {
        name: "area",
        units: &[
            ("m2", 1.0, 0.0),
            ("sqm", 1.0, 0.0),
            ("squaremeter", 1.0, 0.0),
            ("squaremeters", 1.0, 0.0),
            ("km2", 1000000.0, 0.0),
            ("sqkm", 1000000.0, 0.0),
            ("cm2", 0.0001, 0.0),
            ("ft2", 0.09290304, 0.0),
            ("sqft", 0.09290304, 0.0),
            ("in2", 0.00064516, 0.0),
            ("mi2", 2589988.110336, 0.0),
            ("sqmi", 2589988.110336, 0.0),
            ("ha", 10000.0, 0.0),
            ("hectare", 10000.0, 0.0),
            ("ac", 4046.8564224, 0.0),
            ("acre", 4046.8564224, 0.0),
        ],
    },
    UnitCategory {
        name: "pressure",
        units: &[
            ("pa", 1.0, 0.0),
            ("pascal", 1.0, 0.0),
            ("kpa", 1000.0, 0.0),
            ("mpa", 1000000.0, 0.0),
            ("bar", 100000.0, 0.0),
            ("mbar", 100.0, 0.0),
            ("millibar", 100.0, 0.0),
            ("atm", 101325.0, 0.0),
            ("atmosphere", 101325.0, 0.0),
            ("psi", 6894.757293168361, 0.0),
            ("mmhg", 133.322387415, 0.0),
        ],
    },
    UnitCategory {
        name: "time",
        units: &[
            ("s", 1.0, 0.0),
            ("sec", 1.0, 0.0),
            ("second", 1.0, 0.0),
            ("seconds", 1.0, 0.0),
            ("m", 60.0, 0.0),
            ("min", 60.0, 0.0),
            ("minute", 60.0, 0.0),
            ("minutes", 60.0, 0.0),
            ("h", 3600.0, 0.0),
            ("hr", 3600.0, 0.0),
            ("hour", 3600.0, 0.0),
            ("hours", 3600.0, 0.0),
            ("d", 86400.0, 0.0),
            ("day", 86400.0, 0.0),
            ("days", 86400.0, 0.0),
            ("w", 604800.0, 0.0),
            ("week", 604800.0, 0.0),
            ("weeks", 604800.0, 0.0),
        ],
    },
];

fn resolve_unit(alias: &str) -> Result<(&'static str, f64, f64), String> {
    let key = alias
        .to_lowercase()
        .replace('°', "")
        .replace(' ', "");

    let mut matches = Vec::new();

    for category in CATEGORIES {
        for &(unit_name, factor, offset) in category.units {
            if key == unit_name {
                matches.push((category.name, factor, offset));
            }
        }
    }

    if matches.len() > 1 {
        return Err(format!(
            "Ambiguous unit: {}; use an unambiguous name such as meter or min",
            alias
        ));
    }

    if let Some(m) = matches.first() {
        Ok(*m)
    } else {
        Err(format!("Unknown unit: {}", alias))
    }
}

pub fn convert_units_calc(
    value: f64,
    from_unit: &str,
    to_unit: &str,
) -> Result<Value, String> {
    if !value.is_finite() {
        return Err("value must be a finite number".to_string());
    }
    if from_unit.trim().is_empty() || to_unit.trim().is_empty() {
        return Err("from_unit and to_unit are required".to_string());
    }

    let (fcat, ff, fo) = resolve_unit(from_unit)?;
    let (tcat, tf, to_) = resolve_unit(to_unit)?;

    if fcat != tcat {
        return Err(format!(
            "Cannot convert {} ({}) to {} ({})",
            from_unit, fcat, to_unit, tcat
        ));
    }

    let base = value * ff + fo;
    let mut result = (base - to_) / tf;
    if !result.is_finite() {
        return Err("conversion result is not finite".to_string());
    }

    result = (result * 10000.0).round() / 10000.0;

    let res_json = if result.fract() == 0.0 && result.abs() < 1e15 {
        json!(result as i64)
    } else {
        json!(result)
    };

    let val_json = if value.fract() == 0.0 && value.abs() < 1e15 {
        json!(value as i64)
    } else {
        json!(value)
    };

    Ok(json!({
        "value": val_json,
        "from_unit": from_unit,
        "to_unit": to_unit,
        "result": res_json
    }))
}

pub fn convert_units(args: &HashMap<String, Value>) -> Result<Value, String> {
    let val = match args.get("value") {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) => f,
            None => return Err("value must be a finite number".to_string()),
        },
        Some(Value::Bool(_)) => return Err("value must be a finite number".to_string()),
        _ => return Err("value is required".to_string()),
    };

    let from_unit = match args.get("from_unit") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err("from_unit must be a string".to_string()),
    };

    let to_unit = match args.get("to_unit") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err("to_unit must be a string".to_string()),
    };

    convert_units_calc(val, from_unit, to_unit)
}
