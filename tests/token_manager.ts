import * as anchor from "@coral-xyz/anchor";
import { Program, web3 } from "@coral-xyz/anchor";
import { TokenManager } from "../target/types/token_manager.js";
import { PublicKey } from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID, getMint } from "@solana/spl-token";
import { expect } from "chai";

describe("Token Manager Program", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.TokenManager as Program<TokenManager>;

  const [tokenManagerPDA] = PublicKey.findProgramAddressSync(
    [Buffer.from("token-manager"), provider.wallet.publicKey.toBuffer()],
    program.programId,
  );

  describe("Initialization", () => {
    it("should explicitly deploy the TokenManager account and verify deployment", async () => {
      const txSig = await program.methods
        .initializeTokenManager()
        .accounts({
          signer: provider.wallet.publicKey,
          tokenManager: tokenManagerPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .rpc();
      await provider.connection.confirmTransaction(txSig, "confirmed");

      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      expect(tokenManagerAccount.tokens).to.be.an("array").that.is.empty;
    });
  });

  const tokensToCreate = [
    { decimals: 6, isin: "US1234567890" },
    { decimals: 8, isin: "US9876543210" },
    { decimals: 2, isin: "EU1234567890" },
  ];

  describe("Multiple Token Creation", () => {
    tokensToCreate.forEach((tokenData, idx) => {
      it(`should create token share ${idx + 1} with ${tokenData.decimals} decimals and ISIN ${tokenData.isin}`, async () => {
        let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
        const index = tokenManagerAccount.tokens.length;
        const tokenIndexBuffer = Buffer.alloc(8);
        tokenIndexBuffer.writeBigUInt64LE(BigInt(index), 0);

        const [mintPDA] = PublicKey.findProgramAddressSync(
          [Buffer.from("token"), tokenManagerPDA.toBuffer(), tokenIndexBuffer],
          program.programId,
        );

        const txSig = await program.methods
          .createNewShare(tokenData.decimals, tokenData.isin)
          .accounts({
            signer: provider.wallet.publicKey,
            tokenManager: tokenManagerPDA,
            mint: mintPDA,
            tokenProgram: TOKEN_2022_PROGRAM_ID,
            systemProgram: web3.SystemProgram.programId,
          })
          .rpc();
        expect(txSig).to.be.a("string").that.is.not.empty;
        await provider.connection.confirmTransaction(txSig, "confirmed");

        const mintInfo = await getMint(
          provider.connection,
          mintPDA,
          "confirmed",
          TOKEN_2022_PROGRAM_ID
        );
        expect(mintInfo.decimals).to.equal(tokenData.decimals);
        expect(mintInfo.supply.toString()).to.equal("0");

        tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
        expect(tokenManagerAccount.tokens[index].mint.toString()).to.equal(mintPDA.toString());
        expect(tokenManagerAccount.tokens[index].isin).to.equal(tokenData.isin);
      });
    });
  });

  describe("Token Manager Data Verification", () => {
    it("should have the correct total number of tokens deployed", async () => {
      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      expect(tokenManagerAccount.tokens.length).to.equal(tokensToCreate.length);
    });

    it("should verify that each token's stored mint address and ISIN are correctly derived", async () => {
      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      for (let i = 0; i < tokenManagerAccount.tokens.length; i++) {
        const tokenIndexBuffer = Buffer.alloc(8);
        tokenIndexBuffer.writeBigUInt64LE(BigInt(i), 0);
        const [expectedMintPDA] = PublicKey.findProgramAddressSync(
          [Buffer.from("token"), tokenManagerPDA.toBuffer(), tokenIndexBuffer],
          program.programId,
        );
        expect(tokenManagerAccount.tokens[i].mint.toString()).to.equal(expectedMintPDA.toString());
        expect(tokenManagerAccount.tokens[i].isin).to.equal(tokensToCreate[i].isin);
      }
    });
  });
});
