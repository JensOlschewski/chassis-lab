function J = createJMatrix(joint_prim, Binv)
    %CREATEJMATRIX Generates the J matrix based on joint parameters and Binv.
    %
    %   This function takes a structure array representing joints in a robotic
    %   system and the inverse of a B matrix, then constructs the J matrix. 
    %   It collects Hj blocks from each joint, assembles them into a block 
    %   diagonal matrix H, and then multiplies it by Binv to compute J.
    %
    %   Inputs:
    %       joint_prim - A structure array where each element represents a joint.
    %                    Expected to contain a field 'Hj' for each joint.
    %       Binv       - The inverse of a B matrix.
    %
    %   Output:
    %       J - A matrix calculated as the product of Binv and a block diagonal matrix H.
    
        k = length(joint_prim);  % Number of joints
        H_blocks = cell(1, k);   % Initialize a cell array for H blocks
    
        % Collect all Hj blocks
        for i = 1:k
            H_blocks{i} = joint_prim(i).Hj;
        end
    
        % Create H as a block diagonal matrix
        H = createBlkDiag(H_blocks{:});
        
        % Calculate J
        J = Binv * H;
    end
    