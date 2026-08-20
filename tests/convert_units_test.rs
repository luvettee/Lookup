use lookup::tools::convert_units::convert_units_calc;

#[test]
fn test_distance_conversion() {
    let res = convert_units_calc(1.0, "km", "miles").unwrap();
    assert_eq!(res["result"], 0.6214);

    let res2 = convert_units_calc(100.0, "meter", "cm").unwrap();
    assert_eq!(res2["result"], 10000);

    let res3 = convert_units_calc(12.0, "in", "ft").unwrap();
    assert_eq!(res3["result"], 1);
}

#[test]
fn test_temperature_conversion() {
    let res = convert_units_calc(0.0, "C", "F").unwrap();
    assert_eq!(res["result"], 32);

    let res2 = convert_units_calc(212.0, "fahrenheit", "celsius").unwrap();
    assert_eq!(res2["result"], 100);

    let res3 = convert_units_calc(0.0, "celsius", "kelvin").unwrap();
    assert_eq!(res3["result"], 273.15);
}

#[test]
fn test_mass_conversion() {
    let res = convert_units_calc(1.0, "kg", "lbs").unwrap();
    assert_eq!(res["result"], 2.2046);

    let res2 = convert_units_calc(16.0, "oz", "pound").unwrap();
    assert_eq!(res2["result"], 1);
}

#[test]
fn test_storage_conversion() {
    let res = convert_units_calc(1024.0, "mib", "gib").unwrap();
    assert_eq!(res["result"], 1);

    let res2 = convert_units_calc(1.0, "gb", "mb").unwrap();
    assert_eq!(res2["result"], 1000);
}

#[test]
fn test_speed_conversion() {
    let res = convert_units_calc(100.0, "kmh", "mph").unwrap();
    assert_eq!(res["result"], 62.1371);
}

#[test]
fn test_incompatible_units_error() {
    assert!(convert_units_calc(10.0, "km", "kg").is_err());
    assert!(convert_units_calc(10.0, "celsius", "meter").is_err());
    assert!(convert_units_calc(10.0, "invalid_unit", "meter").is_err());
    assert!(convert_units_calc(10.0, "m", "km")
        .unwrap_err()
        .contains("Ambiguous unit"));
}
