use std::path::Path;

use caexfer::conversion::{self, Format, Options, Stage};

#[test]
fn public_conversion_reports_source_and_destination_losses() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
    let mut output = Vec::new();
    let report = conversion::convert_path(&source, Format::Msh, &Options::default(), &mut output)
        .expect("BDF mesh projects to MSH");
    assert_eq!((report.points, report.cells, report.fields), (4, 1, 0));
    assert!(report
        .omissions
        .iter()
        .any(|item| item.stage == Stage::Source));
    assert!(report
        .omissions
        .iter()
        .any(|item| { item.stage == Stage::Destination && item.detail.contains("property IDs") }));
    let dataset = caexfer::msh::read(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(
        (dataset.mesh.points.len(), dataset.mesh.cells.len()),
        (4, 1)
    );
}
