use litcontainers::*;
use litaudio::*;
use litplot::plotly::*;
use std::path::{PathBuf, Path};
use litdsp::*;

pub fn setup_audio() -> AudioDeinterleaved<f64, U1, Dynamic> {
	let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
	path.push("assets/test_audio.wav");
	litaudioio::read_audio(path.as_path()).unwrap()
}

fn main() {
	let audio = setup_audio();

	let bands = littempo::default_audio_bands(audio.sample_rate() as f64);
	let (novelty_curve, sr) = littempo::calculate_novelty_curve(
		&audio,
		audio.sample_rate() as f64,
		Dynamic::new((1024. * audio.sample_rate() as f64 / 22050.) as usize),
		Dynamic::new((512. * audio.sample_rate() as f64 / 22050.) as usize),
		&bands,
		littempo::NCSettingsBuilder::default().build().unwrap()
	);

	let audio_x = litdsp::wave::calculate_time(audio.col_dim(), audio.sample_rate() as f64);

	// Tempogram
	let tempo_window = (8. * sr) as usize;
	let tempo_hop_size = (sr / 5.).ceil() as usize;
	let bpms = RowVec::regspace_rows(U1, D!(571), 30.);
	let (mut tempogram, tempogram_sr) = littempo::novelty_curve_to_tempogram_dft(
		&novelty_curve,
		sr,
		D!(tempo_window),
		D!(tempo_hop_size),
		&bpms
	);
	normalize_cols_inplace(&mut tempogram, |s| norm_p2_c(s));
	let tempogram_mag = (&tempogram).norm();
	let mut tempogram_mag_t = ContainerRM::zeros(tempogram_mag.row_dim(), tempogram_mag.col_dim());
	tempogram_mag_t.copy_from(&tempogram_mag);

	// Cyclic
	let ref_tempo = 60.;
	let (cyclic_tempogram, cyclic_tempogram_axis)
		= littempo::tempogram_to_cyclic_tempogram(&tempogram, &bpms, D!(120), ref_tempo);

	// Preprocess tempogram
	let triplet_weight = 3.;
	let triplet_corrected_cyclic_tempogram = littempo::include_triplets(&cyclic_tempogram, &cyclic_tempogram_axis, triplet_weight);
	let smooth_len = 20.; // 20 sec
	let mut smooth_tempogram = littempo::smoothen_tempogram(
		&triplet_corrected_cyclic_tempogram,
		tempogram_sr,
		smooth_len
	);
	smooth_tempogram.as_iter_mut().for_each(|v| if *v < 0. { *v = 0.; } else {});

	// Tempo curve extraction
	let tempo_curve = littempo::extract_tempo_curve(&smooth_tempogram, &cyclic_tempogram_axis);
	let min_section_length = (10. * tempogram_sr) as usize;
	let tempo_curve = littempo::correct_curve_by_length(&tempo_curve, min_section_length);

	let tempo_segments = littempo::split_curve(&tempo_curve);
	let tempo_sections = littempo::tempo_segments_to_sections(&tempo_curve, &tempo_segments, tempogram_sr, ref_tempo);
	let bpm_merge_threshold = 0.5;
	let tempo_sections_tmp = littempo::merge_sections(&tempo_sections, bpm_merge_threshold);

	let max_section_length = 40.;
	let mut tempo_sections = Vec::new();
	for s in tempo_sections_tmp {
		littempo::split_section(s, &mut tempo_sections, max_section_length);
	}

	let plot = Plot::new("audio")
		.add_chart(
			LineBuilder::default()
				.identifier("audio")
				.data(XYData::new(
					provider_litcontainer(Fetch::Remote, &audio_x, None).unwrap(),
					provider_litcontainer(Fetch::Remote, &audio, None).unwrap(),
				))
				.name("Audio Wave")
				.build()
				.unwrap()
		)
		.add_chart(
			LineBuilder::default()
				.identifier("chart_1")
				.data(XYData::new(
					provider_litcontainer(Fetch::Remote, &litdsp::wave::calculate_time(novelty_curve.col_dim(), sr), Some("chart_1_x".into())).unwrap(),
					provider_litcontainer(Fetch::Remote, &(&novelty_curve / novelty_curve.maximum()), Some("chart_1_y".into())).unwrap(),
				))
				.name("Novelty Curve")
				.build()
				.unwrap()
		);

	let plot2 = Plot::new("tempogram")
		.add_chart(
			HeatmapBuilder::default()
				.data(XYZData::new(
					provider_litcontainer(Fetch::Remote, &litdsp::wave::calculate_time(tempogram.col_dim(), tempogram_sr), None).unwrap(),
					provider_litcontainer(Fetch::Remote, &bpms, None).unwrap(),
					provider_litcontainer(Fetch::Remote, &tempogram_mag_t, None).unwrap(),
				))
				.name("Tempogram")
				.build().unwrap()
		);

	let plot3 = Plot::new("tempogram_cyclic")
		.add_chart(
			HeatmapBuilder::default()
				.data(XYZData::new(
					provider_litcontainer(Fetch::Remote, &litdsp::wave::calculate_time(tempogram.col_dim(), tempogram_sr), None).unwrap(),
					provider_litcontainer(Fetch::Remote, &cyclic_tempogram_axis, None).unwrap(),
					provider_litcontainer(Fetch::Remote, &cyclic_tempogram, None).unwrap(),
				))
				.name("Cyclic Tempogram")
				.build().unwrap()
		);

	let plot4 = Plot::new("smooth_tempogram")
		.add_chart(
			HeatmapBuilder::default()
				.data(XYZData::new(
					provider_litcontainer(Fetch::Remote, &litdsp::wave::calculate_time(tempogram.col_dim(), tempogram_sr), None).unwrap(),
					provider_litcontainer(Fetch::Remote, &cyclic_tempogram_axis, None).unwrap(),
					provider_litcontainer(Fetch::Remote, &smooth_tempogram, None).unwrap(),
				))
				.name("Smooth Tempogram")
				.build().unwrap()
		)
		.add_chart(
			LineBuilder::default()
				.data(XYData::new(
					provider_litcontainer(Fetch::Remote, &litdsp::wave::calculate_time(tempogram.col_dim(), tempogram_sr), None).unwrap(),
					provider_litcontainer(Fetch::Remote, &tempo_curve, None).unwrap()
				))
				.name("Tempo Curve")
				.build().unwrap()
		);

	let report = Report::new("Novelty Curve")
		.add_node(plot)
		.add_node(plot2)
		.add_node(plot3)
		.add_node(plot4);

	let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp").join("novelty_curve");
	report.force_save(path.as_path()).unwrap();
}