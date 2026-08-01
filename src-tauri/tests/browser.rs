//! Comprueba el explorador contra una estructura como la real: una carpeta base con
//! jornadas y, al lado, una carpeta con muchísimos audios viejos que no hay que recorrer.

use std::fs;

use transcriptor_lib::browse;

fn tmpdir(nombre: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("transcriptor-test-{nombre}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("crear temporal");
    d
}

#[test]
fn destaca_las_jornadas_y_no_baja_de_nivel() {
    let base = tmpdir("jornadas");
    for d in ["Lunes 3", "Miércoles 5", "audios viejos", "Entregado 2024"] {
        fs::create_dir_all(base.join(d)).unwrap();
    }
    // Audios metidos un nivel más abajo: no tienen que aparecer al listar la base.
    fs::write(
        base.join("Lunes 3")
            .join("a.mp3"),
        b"x",
    )
    .unwrap();
    fs::write(
        base.join("audios viejos")
            .join("viejo.mp3"),
        b"x",
    )
    .unwrap();
    // Uno suelto en la base sí tiene que aparecer.
    fs::write(base.join("suelto.wav"), b"x").unwrap();

    let l = browse::list(&base).expect("listar");

    assert_eq!(
        l.audios
            .len(),
        1,
        "solo el audio del propio directorio"
    );
    assert_eq!(l.audios[0].name, "suelto.wav");
    assert!(l.has_workdays);

    let jornadas: Vec<&str> = l
        .folders
        .iter()
        .filter(|f| f.workday)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(jornadas.len(), 2);
    assert!(jornadas.contains(&"Lunes 3"));
    assert!(jornadas.contains(&"Miércoles 5"));

    // Las jornadas van primero.
    assert!(
        l.folders[0].workday && l.folders[1].workday,
        "las jornadas tienen que quedar arriba"
    );
    assert!(!l
        .folders
        .last()
        .unwrap()
        .workday);

    let _ = fs::remove_dir_all(&base);
}

/// La carpeta base sin configurar tiene que caer en GoGlobal del escritorio. Esto es lo
/// que hace que en la máquina de ella arranque en C:\Users\Mi PC\Desktop\GoGlobal sin
/// que nadie escriba esa ruta en ningún lado.
#[test]
fn la_base_por_defecto_es_goglobal_del_escritorio() {
    let home = tmpdir("home");
    let goglobal = home
        .join("Desktop")
        .join("GoGlobal");
    fs::create_dir_all(&goglobal).unwrap();

    let previo = std::env::var("USERPROFILE").ok();
    std::env::set_var("USERPROFILE", &home);
    let root = browse::default_root();
    match previo {
        Some(v) => std::env::set_var("USERPROFILE", v),
        None => std::env::remove_var("USERPROFILE"),
    }

    assert_eq!(root, goglobal, "tendría que arrancar en Desktop\\GoGlobal");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn marca_los_que_ya_estan_hechos() {
    let base = tmpdir("hechos");
    fs::write(base.join("uno.mp3"), b"x").unwrap();
    fs::write(base.join("dos.mp3"), b"x").unwrap();
    fs::write(base.join("uno.transcripcion.json"), b"{}").unwrap();

    let l = browse::list(&base).expect("listar");
    let uno = l
        .audios
        .iter()
        .find(|a| a.name == "uno.mp3")
        .unwrap();
    let dos = l
        .audios
        .iter()
        .find(|a| a.name == "dos.mp3")
        .unwrap();
    assert!(uno.done);
    assert!(!dos.done);
    // El .json no es audio: no se lista.
    assert_eq!(
        l.audios
            .len(),
        2
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn una_carpeta_enorme_no_desborda_la_lista() {
    let base = tmpdir("enorme");
    for i in 0..900 {
        fs::write(base.join(format!("viejo-{i}.mp3")), b"x").unwrap();
    }
    let l = browse::list(&base).expect("listar");
    assert_eq!(l.total_audios, 900, "el total se informa completo");
    assert!(
        l.audios
            .len()
            < 900,
        "pero la lista se recorta"
    );

    let _ = fs::remove_dir_all(&base);
}
