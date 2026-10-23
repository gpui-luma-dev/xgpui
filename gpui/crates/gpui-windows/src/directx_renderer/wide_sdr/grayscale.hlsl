// Compatibility path for already-cached ClearType tiles. Collapse coverage before
// correction/blending: no dual-source output or channel-dependent blend factors.
// This is scalar coverage from an LCD tile, not a newly rasterized grayscale glyph.
float4 wide_grayscale_fragment(MonochromeSpriteFragmentInput input) : SV_Target {
    float3 coverage = t_sprite.Sample(s_sprite, input.tile_position).rgb;
    float scalar = dot(coverage, float3(1.0/3.0, 1.0/3.0, 1.0/3.0));
    float alpha = apply_contrast_and_gamma_correction(
        scalar, input.color.rgb, grayscale_enhanced_contrast, gamma_ratios);
    return float4(input.color.rgb, input.color.a * alpha);
}
