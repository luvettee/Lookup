use lookup::tools::calculate::calculate_expression;

#[test]
fn test_basic_arithmetic() {
    let res = calculate_expression("2 + 3 * 4").unwrap();
    assert_eq!(res["result"], 14);

    let res2 = calculate_expression("(10 - 4) / 2").unwrap();
    assert_eq!(res2["result"], 3);

    let res3 = calculate_expression("2 ^ 10").unwrap();
    assert_eq!(res3["result"], 1024);

    let res4 = calculate_expression("2 ** 8").unwrap();
    assert_eq!(res4["result"], 256);

    let res5 = calculate_expression("17 % 5").unwrap();
    assert_eq!(res5["result"], 2);
}

#[test]
fn test_unary_and_constants() {
    let res = calculate_expression("-5 + +10").unwrap();
    assert_eq!(res["result"], 5);

    let res_pi = calculate_expression("pi").unwrap();
    assert!((res_pi["result"].as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-9);

    let res_e = calculate_expression("e").unwrap();
    assert!((res_e["result"].as_f64().unwrap() - std::f64::consts::E).abs() < 1e-9);
}

#[test]
fn test_math_functions() {
    let res_sqrt = calculate_expression("sqrt(144)").unwrap();
    assert_eq!(res_sqrt["result"], 12);

    let res_sin = calculate_expression("sin(0)").unwrap();
    assert_eq!(res_sin["result"], 0);

    let res_cos = calculate_expression("cos(0)").unwrap();
    assert_eq!(res_cos["result"], 1);

    let res_log10 = calculate_expression("log10(1000)").unwrap();
    assert_eq!(res_log10["result"], 3);

    let res_log2 = calculate_expression("log2(64)").unwrap();
    assert_eq!(res_log2["result"], 6);

    let res_log = calculate_expression("log(8, 2)").unwrap();
    assert_eq!(res_log["result"], 3);

    let res_ceil = calculate_expression("ceil(4.2)").unwrap();
    assert_eq!(res_ceil["result"], 5);

    let res_floor = calculate_expression("floor(4.8)").unwrap();
    assert_eq!(res_floor["result"], 4);

    let res_round = calculate_expression("round(3.14159, 2)").unwrap();
    assert_eq!(res_round["result"], 3.14);

    let res_abs = calculate_expression("abs(-42)").unwrap();
    assert_eq!(res_abs["result"], 42);
}

#[test]
fn test_errors() {
    assert!(calculate_expression("1 / 0").is_err());
    assert!(calculate_expression("sqrt(-1)").is_err());
    assert!(calculate_expression("(-2) ^ 0.5").is_err());
    assert!(calculate_expression("2 ^ 1001").is_err()); // Exponent too large
    assert!(calculate_expression("unknown_func(10)").is_err());
    assert!(calculate_expression("").is_err());
}
