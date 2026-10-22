// Test-only adapter to Google's libultrahdr API. Not bundled with the app.
#include "ultrahdr_api.h"
#include <fstream>
#include <vector>
#include <iostream>
#include <iterator>
#include <cmath>
#include <cstdint>
#include <stdexcept>
static void check(uhdr_error_info_t s) {
  if (s.error_code != UHDR_CODEC_OK) throw std::runtime_error(s.has_detail ? s.detail : "Ultra HDR decode failed");
}
int main(int argc,char** argv) {
  if(argc!=2) return 2;
  auto dec=uhdr_create_decoder();
  try {
    std::ifstream in(argv[1],std::ios::binary);
    std::vector<char> bytes((std::istreambuf_iterator<char>(in)),std::istreambuf_iterator<char>());
    uhdr_compressed_image_t image{};image.data=bytes.data();image.data_sz=image.capacity=bytes.size();
    image.cg=UHDR_CG_UNSPECIFIED;image.ct=UHDR_CT_UNSPECIFIED;image.range=UHDR_CR_UNSPECIFIED;
    check(uhdr_dec_set_image(dec,&image));check(uhdr_dec_set_out_color_transfer(dec,UHDR_CT_LINEAR));
    check(uhdr_dec_set_out_img_format(dec,UHDR_IMG_FMT_64bppRGBAHalfFloat));
    check(uhdr_dec_probe(dec));
    const auto* metadata=uhdr_dec_get_gainmap_metadata(dec);
    float boost=metadata->max_content_boost[0];
    check(uhdr_decode(dec));const auto* output=uhdr_get_decoded_image(dec);
    double peak=0;size_t above=0,nonfinite=0;
    for(unsigned y=0;y<output->h;y++) {
      const auto* row=static_cast<const uint16_t*>(output->planes[0])+y*output->stride[0]*4;
      for(unsigned x=0;x<output->w;x++) for(int c=0;c<3;c++) {
        uint16_t h=row[x*4+c];int exp=(h>>10)&31;unsigned frac=h&1023;
        if(exp==31){nonfinite++;continue;}
        double v=exp ? std::ldexp(1.0+double(frac)/1024,exp-15) : std::ldexp(double(frac),-24);
        if(h&0x8000) v=-v;
        peak=std::max(peak,v);if(v>1.001) above++;
      }
    }
    std::cout<<"{\"width\":"<<output->w<<",\"height\":"<<output->h<<",\"max_content_boost\":"<<boost<<",\"linear_rgb_peak\":"<<peak<<",\"channels_above_sdr_white\":"<<above<<",\"nonfinite\":"<<nonfinite<<"}\n";
    uhdr_release_decoder(dec);return nonfinite ? 1 : 0;
  }catch(const std::exception& e){std::cerr<<e.what()<<"\n";uhdr_release_decoder(dec);return 1;}
}
