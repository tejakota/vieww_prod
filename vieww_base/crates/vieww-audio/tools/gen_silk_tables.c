/* Generates src/codec/opusdec/silk/tables.rs from libopus 1.3's own SILK
 * table sources, so every constant is the reference decoder's, compiled.
 *
 *   O=<libopus dir>   # e.g. ~/.cargo/registry/src/.../audiopus_sys-0.2.2/opus
 *   gcc -w -DOPUS_BUILD -I$O/include -I$O/silk -I$O/celt -I$O \
 *       -o /tmp/gen_silk tools/gen_silk_tables.c && /tmp/gen_silk \
 *       > src/codec/opusdec/silk/tables.rs
 *
 * (Run from the crate root with $O on the include path; the table .c
 * files are #included by their path under $O.)
 */
#include <stdio.h>
#include "silk/tables_NLSF_CB_NB_MB.c"
#include "silk/tables_NLSF_CB_WB.c"
#include "silk/tables_LTP.c"
#include "silk/tables_other.c"
#include "silk/tables_gain.c"
#include "silk/tables_pitch_lag.c"
#include "silk/tables_pulses_per_block.c"
#include "silk/table_LSF_cos.c"
#include "silk/pitch_est_tables.c"
#include "silk/resampler_rom.c"
static void pr(const char*name,const char*rty,const void*p,int esz,int sgn,int n){
  printf("pub(crate) const %s: [%s; %d] = [",name,rty,n);
  for(int i=0;i<n;i++){ long v;
    if(esz==1) v= sgn? ((const signed char*)p)[i] : ((const unsigned char*)p)[i];
    else if(esz==2) v=((const short*)p)[i]; else v=((const int*)p)[i];
    printf("%s%ld", i?", ":"", v);}
  printf("];\n");
}
#define U8(N,a) pr(N,"u8",a,1,0,sizeof(a))
#define I8(N,a) pr(N,"i8",a,1,1,sizeof(a))
#define I16(N,a) pr(N,"i16",a,2,1,sizeof(a)/2)
#define I32(N,a) pr(N,"i32",a,4,1,sizeof(a)/4)
int main(){
 printf("//! SILK's constant tables, generated from libopus 1.3 by\n//! tools/gen_silk_tables.c (do not edit). Multi-dimensional tables are\n//! flattened row-major.\n\n#![allow(clippy::unreadable_literal)]\n\n");
 U8("NLSF_CB1_NB_MB_Q8",silk_NLSF_CB1_NB_MB_Q8); I16("NLSF_CB1_WGHT_NB_MB_Q9",silk_NLSF_CB1_Wght_Q9);
 U8("NLSF_CB1_ICDF_NB_MB",silk_NLSF_CB1_iCDF_NB_MB); U8("NLSF_CB2_SELECT_NB_MB",silk_NLSF_CB2_SELECT_NB_MB);
 U8("NLSF_CB2_ICDF_NB_MB",silk_NLSF_CB2_iCDF_NB_MB); U8("NLSF_PRED_NB_MB_Q8",silk_NLSF_PRED_NB_MB_Q8);
 I16("NLSF_DELTA_MIN_NB_MB_Q15",silk_NLSF_DELTA_MIN_NB_MB_Q15);
 U8("NLSF_CB1_WB_Q8",silk_NLSF_CB1_WB_Q8); I16("NLSF_CB1_WGHT_WB_Q9",silk_NLSF_CB1_WB_Wght_Q9);
 U8("NLSF_CB1_ICDF_WB",silk_NLSF_CB1_iCDF_WB); U8("NLSF_CB2_SELECT_WB",silk_NLSF_CB2_SELECT_WB);
 U8("NLSF_CB2_ICDF_WB",silk_NLSF_CB2_iCDF_WB); U8("NLSF_PRED_WB_Q8",silk_NLSF_PRED_WB_Q8);
 I16("NLSF_DELTA_MIN_WB_Q15",silk_NLSF_DELTA_MIN_WB_Q15);
 printf("pub(crate) const NLSF_QSTEP_NB_MB_Q16: i32 = %d;\npub(crate) const NLSF_QSTEP_WB_Q16: i32 = %d;\n", silk_NLSF_CB_NB_MB.quantStepSize_Q16, silk_NLSF_CB_WB.quantStepSize_Q16);
 printf("pub(crate) const NLSF_NVECTORS: usize = %d;\n", silk_NLSF_CB_WB.nVectors);
 U8("LTP_PER_INDEX_ICDF",silk_LTP_per_index_iCDF);
 U8("LTP_GAIN_ICDF_0",silk_LTP_gain_iCDF_0); U8("LTP_GAIN_ICDF_1",silk_LTP_gain_iCDF_1); U8("LTP_GAIN_ICDF_2",silk_LTP_gain_iCDF_2);
 I8("LTP_GAIN_VQ_0",silk_LTP_gain_vq_0); I8("LTP_GAIN_VQ_1",silk_LTP_gain_vq_1); I8("LTP_GAIN_VQ_2",silk_LTP_gain_vq_2);
 I16("STEREO_PRED_QUANT_Q13",silk_stereo_pred_quant_Q13); U8("STEREO_PRED_JOINT_ICDF",silk_stereo_pred_joint_iCDF);
 U8("STEREO_ONLY_CODE_MID_ICDF",silk_stereo_only_code_mid_iCDF);
 U8("LBRR_FLAGS_2_ICDF",silk_LBRR_flags_2_iCDF); U8("LBRR_FLAGS_3_ICDF",silk_LBRR_flags_3_iCDF);
 U8("LSB_ICDF",silk_lsb_iCDF); U8("LTPSCALE_ICDF",silk_LTPscale_iCDF);
 U8("TYPE_OFFSET_VAD_ICDF",silk_type_offset_VAD_iCDF); U8("TYPE_OFFSET_NO_VAD_ICDF",silk_type_offset_no_VAD_iCDF);
 U8("NLSF_INTERPOLATION_FACTOR_ICDF",silk_NLSF_interpolation_factor_iCDF);
 I16("QUANTIZATION_OFFSETS_Q10",silk_Quantization_Offsets_Q10); I16("LTP_SCALES_TABLE_Q14",silk_LTPScales_table_Q14);
 U8("UNIFORM3_ICDF",silk_uniform3_iCDF); U8("UNIFORM4_ICDF",silk_uniform4_iCDF); U8("UNIFORM5_ICDF",silk_uniform5_iCDF);
 U8("UNIFORM6_ICDF",silk_uniform6_iCDF); U8("UNIFORM8_ICDF",silk_uniform8_iCDF); U8("NLSF_EXT_ICDF",silk_NLSF_EXT_iCDF);
 U8("GAIN_ICDF",silk_gain_iCDF); U8("DELTA_GAIN_ICDF",silk_delta_gain_iCDF);
 U8("PITCH_LAG_ICDF",silk_pitch_lag_iCDF); U8("PITCH_DELTA_ICDF",silk_pitch_delta_iCDF);
 U8("PITCH_CONTOUR_ICDF",silk_pitch_contour_iCDF); U8("PITCH_CONTOUR_NB_ICDF",silk_pitch_contour_NB_iCDF);
 U8("PITCH_CONTOUR_10_MS_ICDF",silk_pitch_contour_10_ms_iCDF); U8("PITCH_CONTOUR_10_MS_NB_ICDF",silk_pitch_contour_10_ms_NB_iCDF);
 U8("PULSES_PER_BLOCK_ICDF",silk_pulses_per_block_iCDF); U8("RATE_LEVELS_ICDF",silk_rate_levels_iCDF);
 U8("SHELL_CODE_TABLE0",silk_shell_code_table0); U8("SHELL_CODE_TABLE1",silk_shell_code_table1);
 U8("SHELL_CODE_TABLE2",silk_shell_code_table2); U8("SHELL_CODE_TABLE3",silk_shell_code_table3);
 U8("SHELL_CODE_TABLE_OFFSETS",silk_shell_code_table_offsets); U8("SIGN_ICDF",silk_sign_iCDF);
 I16("LSF_COS_TAB_FIX_Q12",silk_LSFCosTab_FIX_Q12);
 I8("CB_LAGS_STAGE2",silk_CB_lags_stage2); I8("CB_LAGS_STAGE3",silk_CB_lags_stage3);
 I8("CB_LAGS_STAGE2_10_MS",silk_CB_lags_stage2_10_ms); I8("CB_LAGS_STAGE3_10_MS",silk_CB_lags_stage3_10_ms);
 I16("RESAMPLER_FRAC_FIR_12",silk_resampler_frac_FIR_12);
 I16("RESAMPLER_UP2_HQ_0",silk_resampler_up2_hq_0); I16("RESAMPLER_UP2_HQ_1",silk_resampler_up2_hq_1);
 return 0;}
