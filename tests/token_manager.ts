import * as anchor from "@coral-xyz/anchor";
import { Program, web3 } from "@coral-xyz/anchor";
import { TokenManager } from "../target/types/token_manager.js";
import { PublicKey } from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  getMint,
  getAccount,
  createTransferCheckedInstruction,
  getAssociatedTokenAddress,
  createAssociatedTokenAccountInstruction
} from "@solana/spl-token";
import { expect } from "chai";

describe("Token Manager Program", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.TokenManager as Program<TokenManager>;

  const [tokenManagerPDA] = PublicKey.findProgramAddressSync(
    [Buffer.from("token-manager"), provider.wallet.publicKey.toBuffer()],
    program.programId,
  );

  console.log("Token manager PDA:", tokenManagerPDA.toString());

  const tokensToCreate = [
    { decimals: 6, isin: "US1234567890" },
    { decimals: 8, isin: "US9876543210" },
    { decimals: 2, isin: "EU1234567890" },
  ];

  const wallets = {
    authorized: web3.Keypair.generate(),
    unauthorized: web3.Keypair.generate(),
    destination: web3.Keypair.generate()
  };

  const tokenMints = [];

  async function confirmTransaction(signature) {
    await provider.connection.confirmTransaction({
      signature,
      blockhash: (await provider.connection.getLatestBlockhash('confirmed')).blockhash,
      lastValidBlockHeight: (await provider.connection.getLatestBlockhash('confirmed')).lastValidBlockHeight
    }, "confirmed");
  }

  async function fundWallet(wallet, amount = 10000000000) {
    const signature = await provider.connection.requestAirdrop(
      wallet.publicKey,
      amount,
    );
    await confirmTransaction(signature);
  }

  async function createTokenAccount(owner, mint) {
    const tokenAccount = await getAssociatedTokenAddress(
      mint,
      owner.publicKey,
      true,
      TOKEN_2022_PROGRAM_ID,
    );

    try {
      await getAccount(
        provider.connection,
        tokenAccount,
        "confirmed",
        TOKEN_2022_PROGRAM_ID
      );
      return tokenAccount;
    } catch (error) {
      const tx = new web3.Transaction().add(
        createAssociatedTokenAccountInstruction(
          provider.wallet.publicKey,
          tokenAccount,
          owner.publicKey,
          mint,
          TOKEN_2022_PROGRAM_ID,
        )
      );

      await provider.sendAndConfirm(tx);

      return tokenAccount;
    }
  }

  async function getTokenForIsin(isin: string): Promise<{mint: PublicKey, index: anchor.BN}> {
    const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
    const token = tokenManagerAccount.tokens.find(t => t.isin === isin);

    if (!token) {
      throw new Error(`Token with ISIN ${isin} not found`);
    }

    return token;
  }

  before(async () => {
    for (const key in wallets) {
      await fundWallet(wallets[key]);
    }
  });

  describe("1. Program Initialization", () => {
    it("should initialize the TokenManager account", async () => {
      try {
        await program.account.tokenManager.fetch(tokenManagerPDA);
        console.log("TokenManager already initialized, skipping initialization");
      } catch (error) {
        const txSig = await program.methods
          .initializeTokenManager()
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();

        await confirmTransaction(txSig);
      }

      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      expect(tokenManagerAccount.tokens).to.be.an("array");
      expect(tokenManagerAccount.whitelist).to.be.an("array");
      expect(tokenManagerAccount.creator.toString()).to.equal(provider.wallet.publicKey.toString());
    });
  });

  describe("2. Token Creation", () => {
    tokensToCreate.forEach(async (tokenData, idx) => {
      it(`should create token share ${idx + 1} with ${tokenData.decimals} decimals and ISIN ${tokenData.isin}`, async () => {
        let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
        const index = tokenManagerAccount.currentTokenIndex.toNumber();

        console.log("Index:", index);

        const [tokenMintPDA] = PublicKey.findProgramAddressSync(
          [Buffer.from("token-mint"), tokenManagerPDA.toBuffer(), Buffer.from(new BigUint64Array([BigInt(index)]).buffer)],
          program.programId,
        );

        console.log("Token mint PDA:", tokenMintPDA.toString());

        try {
          const txSig = await program.methods
            .createNewShare(
              tokenData.decimals,
              tokenData.isin,
            )
            .accounts({
              signer: provider.wallet.publicKey,
            })
            .rpc();

          await confirmTransaction(txSig);

          tokenMints.push(tokenMintPDA);

          const mintInfo = await getMint(
            provider.connection,
            tokenMintPDA,
            "confirmed",
            TOKEN_2022_PROGRAM_ID
          );

          expect(mintInfo.decimals).to.equal(tokenData.decimals);
          expect(mintInfo.supply.toString()).to.equal("0");

          tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
          const lastIndex = tokenManagerAccount.tokens.length - 1;
          expect(tokenManagerAccount.tokens[lastIndex].mint.toString()).to.equal(tokenMintPDA.toString());
          expect(tokenManagerAccount.tokens[lastIndex].isin).to.equal(tokenData.isin);
        } catch (error) {
          console.log(error);
          if (error.message?.includes("already in use")) {
            console.log(`Token ${tokenData.isin} already exists, skipping creation`);

            tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
            const existingToken = tokenManagerAccount.tokens.find(t => t.isin === tokenData.isin);

            if (existingToken) {
              tokenMints.push(existingToken.mint);
            }
          } else {
            throw error;
          }
        }
      });
    });

    it("should have all tokens correctly stored in TokenManager", async () => {
      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);

      expect(tokenManagerAccount.tokens.length).to.be.at.equal(tokensToCreate.length);

      for (const element of tokensToCreate) {
        const token = tokenManagerAccount.tokens.find(t => t.isin === element.isin);
        expect(token).to.not.be.undefined;
        expect(token.isin).to.equal(element.isin);
      }
    });
  });

  describe("3. Whitelist Management", () => {
    it("should fail when adding a wallet to a non-existent token", async () => {
      const nonExistentIsin = "DOES_NOT_EXIST";

      try {
        await program.methods
          .addToWhitelist(wallets.authorized.publicKey, nonExistentIsin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();
        expect.fail("Expected error when adding to a non-existent token");
      } catch (err: any) {
        expect(err.error.errorCode.code).to.equal("TokenNotFound");
      }
    });

    it("should add a wallet to the whitelist for each token", async () => {
      let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);

      for (const tokenData of tokensToCreate) {
        const existingAuth = tokenManagerAccount.whitelist.find(
          auth => {
            const token = tokenManagerAccount.tokens.find(t => t.isin === tokenData.isin);
            return token &&
                   auth.mint.toString() === token.mint.toString() &&
                   auth.authority.toString() === wallets.authorized.publicKey.toString();
          }
        );

        if (existingAuth) {
          console.log(`Wallet already authorized for ${tokenData.isin}, skipping`);
          continue;
        }

        const txSig = await program.methods
          .addToWhitelist(wallets.authorized.publicKey, tokenData.isin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();

        await confirmTransaction(txSig);
      }

      tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);

      for (const tokenData of tokensToCreate) {
        const token = tokenManagerAccount.tokens.find(t => t.isin === tokenData.isin);
        expect(token).to.not.be.undefined;

        const authorization = tokenManagerAccount.whitelist.find(
          auth => auth.mint.toString() === token.mint.toString() &&
                 auth.authority.toString() === wallets.authorized.publicKey.toString()
        );

        expect(authorization).to.not.be.undefined;
      }
    });

    it("should fail when removing a wallet that is not in the whitelist", async () => {
      const randomWallet = web3.Keypair.generate();
      const validIsin = tokensToCreate[0].isin;

      try {
        await program.methods
          .removeFromWhitelist(randomWallet.publicKey, validIsin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();
        expect.fail("Expected error when removing a wallet not in the whitelist");
      } catch (err: any) {
        expect(err.error.errorCode.code).to.equal("WalletNotFound");
      }
    });
  });

  describe("4. Token Minting", () => {
    let testToken;
    let tokenAccount;

    before(async () => {
      testToken = await getTokenForIsin(tokensToCreate[0].isin);

      tokenAccount = await createTokenAccount(wallets.authorized, testToken.mint);
    });

    it("should mint tokens to test account", async () => {
      const mintAmount = new anchor.BN(1000000);

      const txSig = await program.methods
        .mintTokens(testToken.index, mintAmount)
        .accounts({
          signer: provider.wallet.publicKey,
          destination: tokenAccount,
        })
        .rpc();

      await confirmTransaction(txSig);

      const authBalance = (await getAccount(
        provider.connection,
        tokenAccount,
        "confirmed",
        TOKEN_2022_PROGRAM_ID
      )).amount;

      expect(authBalance.toString()).to.equal(mintAmount.toString());
    });
  });

//   describe("5. Transfer Tests", () => {
//     let testMint;
//     let authorizedTokenAccount;
//     let unauthorizedTokenAccount;
//     let destinationTokenAccount;

//     before(async () => {
//       testMint = tokenMints[0];

//       authorizedTokenAccount = await createTokenAccount(wallets.authorized, testMint);
//       unauthorizedTokenAccount = await createTokenAccount(wallets.unauthorized, testMint);
//       destinationTokenAccount = await createTokenAccount(wallets.destination, testMint);
//     });

//     it("should allow a transfer from a whitelisted wallet", async () => {
//       const preBalanceAuth = (await getAccount(
//         provider.connection,
//         authorizedTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const preBalanceDest = (await getAccount(
//         provider.connection,
//         destinationTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const transferAmount = BigInt(100000);

//       const mintInfo = await getMint(
//         provider.connection,
//         testMint,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       );

//       const transferIx = createTransferCheckedInstruction(
//         authorizedTokenAccount,
//         testMint,
//         destinationTokenAccount,
//         wallets.authorized.publicKey,
//         transferAmount,
//         mintInfo.decimals,
//         [],
//         TOKEN_2022_PROGRAM_ID
//       );

//       const tx = new web3.Transaction().add(transferIx);

//       await web3.sendAndConfirmTransaction(
//         provider.connection,
//         tx,
//         [wallets.authorized],
//         { commitment: "confirmed" }
//       );

//       const postBalanceAuth = (await getAccount(
//         provider.connection,
//         authorizedTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const postBalanceDest = (await getAccount(
//         provider.connection,
//         destinationTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       expect(postBalanceAuth.toString()).to.equal((preBalanceAuth - transferAmount).toString());
//       expect(postBalanceDest.toString()).to.equal((preBalanceDest + transferAmount).toString());
//     });

//     it("should block a transfer from a non-whitelisted wallet", async () => {
//       const preBalanceUnauth = (await getAccount(
//         provider.connection,
//         unauthorizedTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const preBalanceDest = (await getAccount(
//         provider.connection,
//         destinationTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const transferAmount = BigInt(100000);

//       const mintInfo = await getMint(
//         provider.connection,
//         testMint,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       );

//       const transferIx = createTransferCheckedInstruction(
//         unauthorizedTokenAccount,
//         testMint,
//         destinationTokenAccount,
//         wallets.unauthorized.publicKey,
//         transferAmount,
//         mintInfo.decimals,
//         [],
//         TOKEN_2022_PROGRAM_ID
//       );

//       const tx = new web3.Transaction().add(transferIx);

//       try {
//         await web3.sendAndConfirmTransaction(
//           provider.connection,
//           tx,
//           [wallets.unauthorized],
//           { commitment: "confirmed" }
//         );
//         expect.fail("Expected transaction to fail but it succeeded");
//       } catch (error) {
//         expect(error.logs.some(log => log.includes("TransferNotAllowed"))).to.be.true;
//       }

//       const postBalanceUnauth = (await getAccount(
//         provider.connection,
//         unauthorizedTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       const postBalanceDest = (await getAccount(
//         provider.connection,
//         destinationTokenAccount,
//         "confirmed",
//         TOKEN_2022_PROGRAM_ID
//       )).amount;

//       expect(postBalanceUnauth.toString()).to.equal(preBalanceUnauth.toString());
//       expect(postBalanceDest.toString()).to.equal(preBalanceDest.toString());
//     });
//   });

//   describe("6. Additional Token Queries", () => {
//     it("should correctly retrieve token mint by ISIN", async () => {
//       const testIsin = tokensToCreate[0].isin;

//       const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
//       const expectedMint = tokenManagerAccount.tokens.find(t => t.isin === testIsin).mint;

//       const fetchedMint = await program.methods
//         .getToken(testIsin)
//         .accounts({
//           signer: provider.wallet.publicKey
//         })
//         .view();

//       expect(fetchedMint.toString()).to.equal(expectedMint.toString());
//     });

//     it("should fail to retrieve a non-existent token", async () => {
//       const nonExistentIsin = "DOES_NOT_EXIST";

//       try {
//         await program.methods
//           .getToken(nonExistentIsin)
//           .accounts({
//             signer: provider.wallet.publicKey,
//           })
//           .view();
//         expect.fail("Expected error when retrieving a non-existent token");
//       } catch (err: any) {
//         expect(err.error.errorCode.code).to.equal("TokenNotFound");
//       }
//     });
//   });

//   describe("7. Whitelist Management Edge Cases", () => {
//     it("should not allow duplicates in the whitelist", async () => {
//       const validIsin = tokensToCreate[0].isin;

//       await program.methods
//         .addToWhitelist(wallets.authorized.publicKey, validIsin)
//         .accounts({
//           signer: provider.wallet.publicKey,
//         })
//         .rpc();

//       const updatedTokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);

//       const token = updatedTokenManagerAccount.tokens.find(t => t.isin === validIsin);
//       const relevantEntries = updatedTokenManagerAccount.whitelist.filter(
//         auth => auth.mint.toString() === token.mint.toString() &&
//                auth.authority.toString() === wallets.authorized.publicKey.toString()
//       );

//       expect(relevantEntries.length).to.equal(1);
//     });
//   });
});
