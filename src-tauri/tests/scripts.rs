//! Los .ps1 de instalación tienen que sobrevivir a PowerShell 5.1, que es el que trae
//! Windows de fábrica y el que va a usar ella.
//!
//! PowerShell 5.1 lee un script sin BOM como ANSI (cp1252 acá). Un guion largo `—`
//! son los bytes E2 80 94 en UTF-8, y el 0x94 en cp1252 es `”`, que PowerShell acepta
//! como comilla de cierre: la cadena se corta por la mitad y el archivo entero deja de
//! parsear. No falla al guardarlo ni al leerlo, sólo al ejecutarlo en la otra máquina.

use std::path::PathBuf;

fn scripts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("scripts")
}

#[test]
fn los_scripts_de_powershell_llevan_bom() {
    let mut vistos = 0;
    for entrada in std::fs::read_dir(scripts()).expect("no encuentro scripts/") {
        let ruta = entrada.unwrap().path();
        if ruta.extension().and_then(|e| e.to_str()) != Some("ps1") {
            continue;
        }
        vistos += 1;
        let bytes = std::fs::read(&ruta).unwrap();
        assert!(
            bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
            "{} está sin BOM: PowerShell 5.1 lo va a leer como ANSI y no va a parsear. \
             Guardalo como UTF-8 con BOM.",
            ruta.display()
        );
    }
    assert!(vistos >= 3, "esperaba encontrar los scripts, encontré {vistos}");
}

#[test]
fn el_bat_que_baja_los_modelos_apunta_a_un_archivo_que_existe() {
    // El .bat se copia a la entrega con otro nombre; lo que no puede pasar es que
    // apunte a un .ps1 que nadie genera.
    let bat = std::fs::read_to_string(scripts().join("descargar-modelos.bat")).unwrap();
    let empaqueta = std::fs::read_to_string(scripts().join("package.ps1")).unwrap();
    assert!(bat.contains("descargar-modelos.ps1"));
    assert!(
        empaqueta.contains("\"descargar-modelos.ps1\""),
        "el .bat llama a descargar-modelos.ps1 pero package.ps1 no lo deja en la entrega"
    );
}
