import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { TokenManager } from "../target/types/token_manager.js";
import { PublicKey } from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID, getMint } from "@solana/spl-token";
import { expect } from "chai";

describe("token_manager", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.TokenManager as Program<TokenManager>;

  let mint: PublicKey;
  let txSignature: string;

  before(() => {
    [mint] = PublicKey.findProgramAddressSync(
      [Buffer.from("my-mint")],
      program.programId
    );
  });

  describe("Deployment", () => {
    it("should have the program deployed", () => {
      expect(program.programId).to.be.instanceOf(PublicKey);
    });
  });

  describe("Method Calls", () => {
    it("should create a new token with 6 decimals", async () => {
      txSignature = await program.methods
        .createNewShare(6)
        .accounts({
          authority: provider.wallet.publicKey,
          mint,
          systemProgram: anchor.web3.SystemProgram.programId,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .rpc();

      expect(txSignature).to.be.a("string").that.is.not.empty;

      await provider.connection.confirmTransaction(txSignature, "confirmed");
    });
  });

  describe("Token SPL Verification", () => {
    it("should retrieve the token mint with the correct number of decimals", async () => {
      const mintInfo = await getMint(
        provider.connection,
        mint,
        "confirmed",
        TOKEN_2022_PROGRAM_ID
      );
      expect(mintInfo.decimals).to.equal(6);
    });

    it("should have an initial supply of 0", async () => {
      const mintInfo = await getMint(
        provider.connection,
        mint,
        "confirmed",
        TOKEN_2022_PROGRAM_ID
      );
      expect(mintInfo.supply.toString()).to.equal("0");
    });
  });
});
