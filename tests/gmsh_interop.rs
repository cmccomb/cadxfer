//! Opt-in external Gmsh import gate, enabled by `CAEXFER_GMSH=gmsh`.
use caexfer::core::{Cell, CellKind, Dataset, Mesh, Point};
use caexfer::msh;
use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

fn cell_tags(mesh: &Mesh) -> BTreeMap<u64, (CellKind, Vec<u64>)> {
    mesh.cells
        .iter()
        .map(|cell| {
            (
                cell.id,
                (
                    cell.kind,
                    cell.connectivity
                        .iter()
                        .map(|&index| mesh.points[index].id)
                        .collect(),
                ),
            )
        })
        .collect()
}

#[test]
fn gmsh_import_preserves_mixed_dimensions_and_original_ids() {
    let Ok(gmsh) = std::env::var("CAEXFER_GMSH") else {
        return;
    };
    let points = vec![
        Point {
            id: 10,
            position: [0., 0., 0.],
        },
        Point {
            id: 20,
            position: [1., 0., 0.],
        },
        Point {
            id: 30,
            position: [0., 1., 0.],
        },
        Point {
            id: 40,
            position: [0., 0., 1.],
        },
    ];
    let cases = [
        (
            "mixed",
            vec![
                Cell {
                    id: 100,
                    kind: CellKind::Line2,
                    connectivity: vec![0, 1],
                    property_id: None,
                },
                Cell {
                    id: 200,
                    kind: CellKind::Triangle3,
                    connectivity: vec![0, 1, 2],
                    property_id: None,
                },
                Cell {
                    id: 300,
                    kind: CellKind::Tet4,
                    connectivity: vec![0, 1, 2, 3],
                    property_id: None,
                },
            ],
        ),
        ("nodes-only", vec![]),
    ];
    for (name, cells) in cases {
        let dataset = Dataset {
            mesh: Mesh {
                points: points.clone(),
                cells,
            },
            fields: vec![],
        };
        let directory = std::env::temp_dir();
        let prefix = format!("caexfer-gmsh-{}-{name}", std::process::id());
        let source = directory.join(format!("{prefix}.msh"));
        let resaved = directory.join(format!("{prefix}-resaved.msh"));
        let mut bytes = Vec::new();
        msh::write(&dataset, &mut bytes).unwrap();
        fs::write(&source, bytes).unwrap();
        let output = Command::new(&gmsh)
            .arg(&source)
            .args(["-0", "-o"])
            .arg(&resaved)
            .args(["-v", "3"])
            .output()
            .expect("launch Gmsh");
        assert!(
            output.status.success(),
            "Gmsh rejected {name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let external = fs::read_to_string(&resaved).expect("Gmsh must write ASCII MSH");
        let imported = msh::read(&external).expect("read Gmsh-resaved MSH");
        let original_points: BTreeMap<_, _> = dataset
            .mesh
            .points
            .iter()
            .map(|p| (p.id, p.position))
            .collect();
        let imported_points: BTreeMap<_, _> = imported
            .mesh
            .points
            .iter()
            .map(|p| (p.id, p.position))
            .collect();
        assert_eq!(
            imported_points, original_points,
            "Gmsh changed point IDs or coordinates"
        );
        assert_eq!(
            cell_tags(&imported.mesh),
            cell_tags(&dataset.mesh),
            "Gmsh changed element IDs or connectivity"
        );
        fs::remove_file(source).unwrap();
        fs::remove_file(resaved).unwrap();
    }
}
