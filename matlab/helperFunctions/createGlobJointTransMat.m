function [TiTransposeBlkDiag,TjTransposeBlkDiag] = createGlobJointTransMat (conStruct)
    % ##################################################################################
    % Function: createGlobJointTransMat
    %
    % Description:
    %   Generates global transformation matrices by creating block diagonal matrices
    %   from individual transformation matrices within a mechanical system.
    %
    % Inputs:
    %   conStruct - Structure array containing transformation matrices with fields:
    %               - Ti: Transformation matrices from reference frames to joints (as cell arrays)
    %               - Tj: Transformation matrices from reference frames to joints (as cell arrays)
    %
    % Outputs:
    %   TiTransposeBlkDiag - Block diagonal matrix of transposed Ti matrices
    %   TjTransposeBlkDiag - Block diagonal matrix of transposed Tj matrices
    % ##################################################################################
    % Transpose the Ti matrices
    TiTransposed = arrayfun(@(x) x.Ti', conStruct, 'UniformOutput', false);
    
    % Create block diagonal matrix from transposed Ti matrices
    TiTransposeBlkDiag = blkdiag(TiTransposed{:});
    
    % Transpose the Tj matrices
    TjTransposed = arrayfun(@(x) x.Tj', conStruct, 'UniformOutput', false);
    
    % Create block diagonal matrix from transposed Tj matrices
    TjTransposeBlkDiag = blkdiag(TjTransposed{:});

end





