## TimingStatistics - Shared timing statistics computation
##
## Computes comprehensive timing metrics from frame delta arrays.
## Used by both CameraDataset and StimulusDataset for uniform statistics.
class_name TimingStatistics
extends RefCounted


## Compute full statistics from frame timing data.
##
## Returns a dictionary with:
## - Frame timing: mean_delta_us, min_delta_us, max_delta_us, jitter_us
## - Drift: total_drift_us, mean_drift_us, drift_rate_ppm
## - Drops: drop_count, drop_rate_per_min
## - Session: frame_count, elapsed_us, actual_fps, expected_fps
static func compute(
	deltas: PackedInt64Array,
	expected_delta_us: int,
	dropped_indices: PackedInt32Array,
	first_ts_us: int,
	last_ts_us: int
) -> Dictionary:
	var n := deltas.size()

	if n == 0:
		return {"frame_count": 0}

	var result := {}

	# Basic counts
	result["frame_count"] = n + 1  # deltas = frames - 1
	result["drop_count"] = dropped_indices.size()

	# Elapsed time
	var elapsed_us: int = last_ts_us - first_ts_us
	result["elapsed_us"] = elapsed_us

	# FPS
	if elapsed_us > 0:
		result["actual_fps"] = float(n + 1) * 1000000.0 / float(elapsed_us)
	else:
		result["actual_fps"] = 0.0

	if expected_delta_us > 0:
		result["expected_fps"] = 1000000.0 / float(expected_delta_us)
	else:
		result["expected_fps"] = 0.0

	# Delta statistics: min, max, mean, SD (jitter)
	var sum: int = 0
	var sum_sq: int = 0
	var min_val: int = deltas[0]
	var max_val: int = deltas[0]

	for i in range(n):
		var delta: int = deltas[i]
		sum += delta
		sum_sq += delta * delta
		if delta < min_val:
			min_val = delta
		if delta > max_val:
			max_val = delta

	var mean: float = float(sum) / float(n)
	var variance: float = (float(sum_sq) / float(n)) - (mean * mean)

	result["min_delta_us"] = min_val
	result["max_delta_us"] = max_val
	result["mean_delta_us"] = mean
	result["jitter_us"] = sqrt(maxf(0.0, variance))

	# Drift statistics (deviation from expected timing)
	if expected_delta_us > 0:
		var expected_elapsed_us: int = n * expected_delta_us
		var total_drift_us: int = elapsed_us - expected_elapsed_us
		result["total_drift_us"] = total_drift_us
		result["mean_drift_us"] = float(total_drift_us) / float(n)

		if expected_elapsed_us > 0:
			result["drift_rate_ppm"] = (float(total_drift_us) / float(expected_elapsed_us)) * 1000000.0
		else:
			result["drift_rate_ppm"] = 0.0
	else:
		result["total_drift_us"] = 0
		result["mean_drift_us"] = 0.0
		result["drift_rate_ppm"] = 0.0

	# Drop rate (drops per minute)
	var elapsed_min: float = float(elapsed_us) / 60000000.0
	if elapsed_min > 0:
		result["drop_rate_per_min"] = float(dropped_indices.size()) / elapsed_min
	else:
		result["drop_rate_per_min"] = 0.0

	return result
