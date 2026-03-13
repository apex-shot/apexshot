"use client";

import { Mail, MessageSquare, Twitter, Github } from "lucide-react";

export default function ContactPage() {
    return (
        <section className="relative min-h-[120vh] bg-[#000000] flex flex-col items-center pt-32 pb-20 overflow-hidden font-sans w-full">

            {/* Background massive 'Contact' text */}
            <div className="absolute top-[120px] md:top-[140px] left-1/2 -translate-x-1/2 w-full flex justify-center pointer-events-none select-none z-0">
                <h2
                    className="text-[120px] md:text-[200px] lg:text-[280px] font-bold text-white/[0.03] leading-none tracking-tighter"
                >
                    Contact
                </h2>
            </div>

            <div className="relative z-10 w-full max-w-6xl px-4 mt-32 md:mt-40 lg:mt-48">

                <div className="grid lg:grid-cols-5 gap-4 lg:gap-6 max-w-[1100px] mx-auto">

                    {/* Contact Info Sidebar - Matches Pricing Card Style */}
                    <div className="lg:col-span-2 group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-10 flex flex-col gap-10 min-h-[460px] shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]">
                        <div>
                            <h1 className="text-4xl lg:text-[44px] font-semibold tracking-tight text-white mb-4">
                                Get in touch
                            </h1>
                            <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed">
                                Questions about features, pricing, or need support? Our team is here to help you out.
                            </p>
                        </div>

                        <div className="w-full h-px bg-white/[0.06]" />

                        <div className="flex flex-col gap-8 flex-1">
                            <div className="space-y-4">
                                <h3 className="text-[13px] text-[#A1A1AA] font-normal tracking-wide">Direct Contact</h3>
                                <a href="mailto:hello@apexshot.com" className="flex items-center gap-4 text-white group/link transition-colors cursor-pointer">
                                    <div className="w-10 h-10 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 border border-white/[0.05] group-hover/link:bg-white/[0.08] transition-colors">
                                        <Mail size={16} className="text-[#A1A1AA] group-hover/link:text-white transition-colors" />
                                    </div>
                                    <span className="text-[15px] font-light">hello@apexshot.com</span>
                                </a>
                            </div>

                            <div className="space-y-4">
                                <h3 className="text-[13px] text-[#A1A1AA] font-normal tracking-wide">Community & Support</h3>
                                <a href="https://discord.com" className="flex items-center gap-4 text-white group/link transition-colors cursor-pointer">
                                    <div className="w-10 h-10 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 border border-white/[0.05] group-hover/link:bg-white/[0.08] transition-colors">
                                        <MessageSquare size={16} className="text-[#A1A1AA] group-hover/link:text-white transition-colors" />
                                    </div>
                                    <div className="flex flex-col">
                                        <span className="text-[15px] font-light">Join our Discord</span>
                                    </div>
                                </a>
                                <a href="https://github.com" className="flex items-center gap-4 text-white group/link transition-colors cursor-pointer">
                                    <div className="w-10 h-10 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 border border-white/[0.05] group-hover/link:bg-white/[0.08] transition-colors">
                                        <Github size={16} className="text-[#A1A1AA] group-hover/link:text-white transition-colors" />
                                    </div>
                                    <div className="flex flex-col">
                                        <span className="text-[15px] font-light">GitHub Discussions</span>
                                    </div>
                                </a>
                            </div>

                            <div className="space-y-4">
                                <h3 className="text-[13px] text-[#A1A1AA] font-normal tracking-wide">Social</h3>
                                <a href="https://twitter.com" className="flex items-center gap-4 text-white group/link transition-colors cursor-pointer">
                                    <div className="w-10 h-10 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 border border-white/[0.05] group-hover/link:bg-white/[0.08] transition-colors">
                                        <Twitter size={16} className="text-[#A1A1AA] group-hover/link:text-white transition-colors" />
                                    </div>
                                    <span className="text-[15px] font-light">@ApexShotApp</span>
                                </a>
                            </div>
                        </div>
                    </div>

                    {/* Contact Form - Matches Pricing Card Style */}
                    <div className="lg:col-span-3 group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-12 flex flex-col min-h-[460px] shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]">
                        <form className="flex flex-col gap-8 h-full">

                            <div className="grid md:grid-cols-2 gap-8">
                                <div className="flex flex-col gap-2">
                                    <label htmlFor="firstName" className="text-[13px] font-medium text-[#A1A1AA] tracking-wide ml-1">First Name</label>
                                    <input
                                        type="text"
                                        id="firstName"
                                        placeholder="Jane"
                                        className="w-full bg-[#18181b]/50 border border-white/[0.05] rounded-2xl px-5 py-4 text-[15px] text-white focus:outline-none focus:border-white/[0.2] focus:bg-white/[0.05] transition-colors placeholder:text-[#52525b]"
                                    />
                                </div>
                                <div className="flex flex-col gap-2">
                                    <label htmlFor="lastName" className="text-[13px] font-medium text-[#A1A1AA] tracking-wide ml-1">Last Name</label>
                                    <input
                                        type="text"
                                        id="lastName"
                                        placeholder="Doe"
                                        className="w-full bg-[#18181b]/50 border border-white/[0.05] rounded-2xl px-5 py-4 text-[15px] text-white focus:outline-none focus:border-white/[0.2] focus:bg-white/[0.05] transition-colors placeholder:text-[#52525b]"
                                    />
                                </div>
                            </div>

                            <div className="flex flex-col gap-2">
                                <label htmlFor="email" className="text-[13px] font-medium text-[#A1A1AA] tracking-wide ml-1">Work Email</label>
                                <input
                                    type="email"
                                    id="email"
                                    placeholder="jane@example.com"
                                    className="w-full bg-[#18181b]/50 border border-white/[0.05] rounded-2xl px-5 py-4 text-[15px] text-white focus:outline-none focus:border-white/[0.2] focus:bg-white/[0.05] transition-colors placeholder:text-[#52525b]"
                                />
                            </div>

                            <div className="flex flex-col gap-2">
                                <label htmlFor="subject" className="text-[13px] font-medium text-[#A1A1AA] tracking-wide ml-1">Subject</label>
                                <div className="relative">
                                    <select
                                        id="subject"
                                        className="w-full appearance-none bg-[#18181b]/50 border border-white/[0.05] rounded-2xl px-5 py-4 text-[15px] text-white focus:outline-none focus:border-white/[0.2] focus:bg-white/[0.05] transition-colors focus:ring-0"
                                        defaultValue=""
                                    >
                                        <option value="" disabled className="text-[#52525b]">How can we help you?</option>
                                        <option value="sales">Sales & Pricing</option>
                                        <option value="support">Technical Support</option>
                                        <option value="press">Press & Media</option>
                                        <option value="other">Other</option>
                                    </select>
                                    <div className="pointer-events-none absolute inset-y-0 right-5 flex items-center text-[#A1A1AA]">
                                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                                            <polyline points="6 9 12 15 18 9"></polyline>
                                        </svg>
                                    </div>
                                </div>
                            </div>

                            <div className="flex flex-col gap-2 flex-1">
                                <label htmlFor="message" className="text-[13px] font-medium text-[#A1A1AA] tracking-wide ml-1">Message</label>
                                <textarea
                                    id="message"
                                    placeholder="Tell us more about your inquiry..."
                                    className="w-full h-full min-h-[160px] bg-[#18181b]/50 border border-white/[0.05] rounded-2xl px-5 py-4 text-[15px] text-white focus:outline-none focus:border-white/[0.2] focus:bg-white/[0.05] transition-colors placeholder:text-[#52525b] resize-none"
                                ></textarea>
                            </div>

                            <div className="pt-2">
                                <button
                                    type="button"
                                    className="w-full flex items-center justify-center h-14 bg-[#F4F4F5] text-black rounded-full font-medium transition-colors hover:bg-white"
                                >
                                    Send Message
                                </button>
                            </div>

                        </form>
                    </div>

                </div>
            </div>
        </section>
    );
}
